//! Telemetri: strukturert logging til Cloud Logging og spans til OTLP.
//!
//! To signaler, én sannhet. Hver loggpost bærer `logging.googleapis.com/trace`
//! slik at Cloud Logging og Cloud Trace viser samme hendelsesforløp, og
//! trace-konteksten følger meldingen videre over NATS.

mod cloud_logging;

use std::sync::OnceLock;

use async_nats::HeaderMap;
use opentelemetry::propagation::{Extractor, Injector};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry::{KeyValue, global};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::{Span, Subscriber, subscriber::set_global_default};
use tracing_log::LogTracer;
use tracing_opentelemetry::{OpenTelemetryLayer, OpenTelemetrySpanExt};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Layer, Registry};

pub use cloud_logging::gcp_project_id;

/// Batch-eksportøren buffrer. Uten en eksplisitt shutdown går siste batch tapt
/// ved SIGTERM — typisk nettopp den som forklarer hvorfor tjenesten stoppet.
static TRACER_PROVIDER: OnceLock<SdkTracerProvider> = OnceLock::new();

pub fn init_observability() {
    let (subscriber, oppsett) = build_subscriber();
    init_subscriber(subscriber);
    oppsett.logg();
}

pub fn init_subscriber(subscriber: impl Subscriber + Send + Sync) {
    // Feiler hvis en logger allerede er satt, som er greit: da er `log`-broen
    // allerede på plass.
    let _ = LogTracer::init();
    set_global_default(subscriber).expect("Failed to set subscriber")
}

pub fn get_subscriber() -> impl Subscriber + Sync + Send {
    build_subscriber().0
}

/// Tømmer trace-bufferet. Kalles ved kontrollert nedstenging.
pub fn shutdown_telemetry() {
    let Some(provider) = TRACER_PROVIDER.get() else {
        return;
    };
    if let Err(err) = provider.shutdown() {
        tracing::warn!(error = %err, "kunne ikke tømme trace-eksporten ved nedstenging");
    }
}

/// Det oppstarten fant ut, logget først når subscriberen er på plass.
struct Telemetrioppsett {
    service_name: String,
    service_version: String,
    otlp_endpoint: Option<String>,
    eksportfeil: Option<String>,
    project_id: Option<String>,
}

impl Telemetrioppsett {
    fn logg(&self) {
        // Endepunktet kommer fra miljøet og kan bære legitimasjon i
        // authority-delen. Operatøren trenger å vite hvor spans sendes, ikke
        // hva de sendes med.
        let endepunkt = self
            .otlp_endpoint
            .as_deref()
            .map(|url| crate::url_etikett::trygg_url_etikett(url, "http"));

        match (&endepunkt, &self.eksportfeil) {
            (Some(endpoint), None) => tracing::info!(
                service_name = %self.service_name,
                service_version = %self.service_version,
                otlp_endpoint = %endpoint,
                gcp_project_id = ?self.project_id,
                "telemetri: spans eksporteres til OTLP"
            ),
            (Some(endpoint), Some(feil)) => {
                tracing::error!(
                    otlp_endpoint = %endpoint,
                    "telemetri: OTLP-eksportøren kunne ikke bygges, spans eksporteres ikke"
                );
                // Byggefeilen gjengir gjerne hele URL-en, inkludert det
                // etiketten over utelater. Samme deling som for Sikri-feil.
                tracing::debug!(error = %feil, "telemetri: byggefeil fra OTLP-eksportøren");
            }
            // Uten endpoint blir loggene stående alene: fortsatt strukturerte,
            // men uten trace å slå opp i.
            (None, _) => tracing::warn!(
                service_name = %self.service_name,
                "telemetri: OTEL_EXPORTER_OTLP_ENDPOINT er ikke satt, spans eksporteres ikke"
            ),
        }

        if self.project_id.is_none() {
            tracing::warn!(
                "telemetri: GOOGLE_CLOUD_PROJECT er ikke satt, loggposter kobles ikke til Cloud Trace"
            );
        }
    }
}

fn build_subscriber() -> (impl Subscriber + Sync + Send, Telemetrioppsett) {
    let service_name = service_name();
    let service_version = service_version();
    let project_id = gcp_project_id();
    let otlp_endpoint = otlp_endpoint();

    let resource = Resource::builder()
        .with_service_name(service_name.clone())
        .with_attributes(resource_attributes(&service_version, project_id.as_deref()))
        .build();

    installer_propagator();

    let mut eksportfeil = None;
    let otel_layer =
        otlp_endpoint
            .as_deref()
            .and_then(|endpoint| match tracer_provider(endpoint, resource) {
                Ok(provider) => {
                    global::set_tracer_provider(provider.clone());
                    let tracer = provider.tracer("skuffen");
                    let _ = TRACER_PROVIDER.set(provider);
                    Some(OpenTelemetryLayer::new(tracer).with_filter(trace_filter()))
                }
                Err(err) => {
                    eksportfeil = Some(err.to_string());
                    None
                }
            });

    let subscriber = Registry::default()
        .with(otel_layer)
        .with(cloud_logging::layer().with_filter(log_filter()));

    (
        subscriber,
        Telemetrioppsett {
            service_name,
            service_version,
            otlp_endpoint,
            eksportfeil,
            project_id,
        },
    )
}

/// Kun trace context. Baggage er bevisst utelatt: ingenting i Skuffen setter
/// eller leser den, så den ville bare gjort tjenesten til et relé for
/// kallerkontrollerte nøkkel-verdi-par inn i våre egne subjects.
/// Forretningsnøkkelen bæres av `correlation_id`, som i tillegg overlever ack,
/// restart og et døgn med retries.
///
/// Testene kaller denne, ikke sin egen variant, slik at de låser det oppsettet
/// produksjonen faktisk kjører.
fn installer_propagator() {
    global::set_text_map_propagator(TraceContextPropagator::new());
}

fn tracer_provider(endpoint: &str, resource: Resource) -> anyhow::Result<SdkTracerProvider> {
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;

    Ok(SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build())
}

fn service_name() -> String {
    env_verdi("OTEL_SERVICE_NAME")
        .or_else(|| env_verdi("K_SERVICE"))
        .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string())
}

/// Cargo-versjonen er den samme i alle revisjoner. `K_REVISION` er det som
/// faktisk skiller to utrullinger fra hverandre.
fn service_version() -> String {
    env_verdi("SKUFFEN_VERSION")
        .or_else(|| env_verdi("K_REVISION"))
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

fn resource_attributes(service_version: &str, project_id: Option<&str>) -> Vec<KeyValue> {
    let mut attributter = vec![KeyValue::new(
        "service.version",
        service_version.to_string(),
    )];

    if let Some(miljo) = env_verdi("APP_ENV") {
        attributter.push(KeyValue::new("deployment.environment.name", miljo));
    }
    // Telemetry API-et trenger prosjektet eksplisitt når spans sendes dit.
    if let Some(project_id) = project_id {
        attributter.push(KeyValue::new("gcp.project_id", project_id.to_string()));
    }

    attributter
}

fn otlp_endpoint() -> Option<String> {
    env_verdi("OTEL_EXPORTER_OTLP_TRACES_ENDPOINT")
        .or_else(|| env_verdi("OTEL_EXPORTER_OTLP_ENDPOINT"))
}

fn log_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

/// Eget filter for spans. Ellers ville `RUST_LOG=warn` også fjernet spanene, og
/// dermed trace-konteksten loggene korrelerer mot.
fn trace_filter() -> EnvFilter {
    EnvFilter::try_from_env("SKUFFEN_TRACE_FILTER").unwrap_or_else(|_| EnvFilter::new("info"))
}

fn env_verdi(navn: &str) -> Option<String> {
    std::env::var(navn)
        .ok()
        .map(|verdi| verdi.trim().to_string())
        .filter(|verdi| !verdi.is_empty())
}

// ---------------------------------------------------------------------------
// Feltkonvensjoner
// ---------------------------------------------------------------------------

/// Setter `correlation_id` på gjeldende span som ren streng.
///
/// Feltet er nøkkelen til hele forløpet på tvers av kommandoer og traces, så
/// verdien må være søkbar som den er. En `Option` formatert med `?` ville gitt
/// «Some(...)», som ingen finner igjen.
pub fn record_correlation_id(correlation_id: Option<uuid::Uuid>) {
    if let Some(correlation_id) = correlation_id {
        Span::current().record("correlation_id", tracing::field::display(correlation_id));
    }
}

// ---------------------------------------------------------------------------
// Trace context ut
// ---------------------------------------------------------------------------

/// Trace-konteksten for gjeldende span, klar til å legges på en utgående
/// melding. `None` når det ikke finnes noe span å videreføre.
///
/// Konteksten hentes fra spanet, ikke fra `opentelemetry::Context::current()`:
/// `tracing-opentelemetry` attacher aldri konteksten, så den globale er tom.
pub fn trace_headers() -> Option<HeaderMap> {
    let context = Span::current().context();
    let mut injector = HeaderInjector {
        headers: HeaderMap::new(),
        antall: 0,
    };

    global::get_text_map_propagator(|propagator| {
        propagator.inject_context(&context, &mut injector);
    });

    (injector.antall > 0).then_some(injector.headers)
}

struct HeaderInjector {
    headers: HeaderMap,
    antall: usize,
}

impl Injector for HeaderInjector {
    fn set(&mut self, key: &str, value: String) {
        self.headers.insert(key, value);
        self.antall += 1;
    }
}

// ---------------------------------------------------------------------------
// Trace context inn
// ---------------------------------------------------------------------------

/// Setter innkommende trace-kontekst som parent på et span som ennå ikke er
/// aktivert.
///
/// `tracing-opentelemetry` bygger OTel-spanet ved `on_enter`, så en
/// `set_parent` fra innsiden av en `#[instrument]`-kropp blir stille ignorert.
/// Handlere som skal videreføre avsenderens trace må derfor lage spanet, sette
/// parent, og først da instrumentere arbeidet med det.
pub fn set_parent_on_span_from_nats_headers(span: &Span, headers: Option<&HeaderMap>) {
    let context = match headers {
        Some(headers) => {
            global::get_text_map_propagator(|prop| prop.extract(&HeaderMapExtractor(headers)))
        }
        None => opentelemetry::Context::new(),
    };
    let _ = span.set_parent(context);
}

struct HeaderMapExtractor<'a>(&'a HeaderMap);

impl Extractor for HeaderMapExtractor<'_> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|verdi| verdi.as_str())
    }

    fn keys(&self) -> Vec<&str> {
        vec!["traceparent", "tracestate"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::TraceContextExt;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing_subscriber::layer::SubscriberExt;

    const INNKOMMENDE_TRACE_ID: &str = "0af7651916cd43dd8448eb211c80319c";

    fn innkommende_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "traceparent",
            format!("00-{INNKOMMENDE_TRACE_ID}-b7ad6b7169203331-01"),
        );
        headers
    }

    /// Kjører `arbeid` med et OTel-lag på plass, slik testen ser de samme
    /// trace-id-ene som produksjon ville gjort.
    fn med_otel(arbeid: impl FnOnce()) {
        installer_propagator();
        let provider = SdkTracerProvider::builder().build();
        let subscriber = Registry::default()
            .with(OpenTelemetryLayer::new(provider.tracer("test")))
            .with(EnvFilter::new("info"));
        tracing::subscriber::with_default(subscriber, arbeid);
    }

    fn trace_id_for(span: &Span) -> String {
        span.context().span().span_context().trace_id().to_string()
    }

    #[test]
    fn parent_satt_for_aktivering_viderefoerer_avsenderens_trace() {
        med_otel(|| {
            let span = tracing::info_span!("command.validate");
            set_parent_on_span_from_nats_headers(&span, Some(&innkommende_headers()));

            assert_eq!(trace_id_for(&span), INNKOMMENDE_TRACE_ID);
        });
    }

    /// Regresjonsvakt: `tracing-opentelemetry` bygger OTel-spanet ved
    /// aktivering. Settes parent etterpå — slik en `#[instrument]`-kropp gjør —
    /// blir avsenderens trace stille forkastet, og løpet brytes i to.
    #[test]
    fn parent_satt_etter_aktivering_blir_forkastet() {
        med_otel(|| {
            let span = tracing::info_span!("command.validate");
            let _guard = span.enter();
            set_parent_on_span_from_nats_headers(&span, Some(&innkommende_headers()));

            assert_ne!(trace_id_for(&span), INNKOMMENDE_TRACE_ID);
        });
    }

    #[test]
    fn utgaaende_headere_baerer_gjeldende_span() {
        med_otel(|| {
            let span = tracing::info_span!("nats.publish");
            let _guard = span.enter();

            let headers = trace_headers().expect("aktivt span gir traceparent");
            let traceparent = headers
                .get("traceparent")
                .expect("traceparent er satt")
                .as_str()
                .to_string();

            assert!(traceparent.contains(&trace_id_for(&span)));
        });
    }

    /// Baggage er bevisst ikke propagert. En kaller skal ikke kunne bruke
    /// Skuffen som relé for egne nøkkel-verdi-par inn i våre subjects.
    #[test]
    fn baggage_fra_kaller_videreformidles_ikke() {
        med_otel(|| {
            let mut headers = innkommende_headers();
            headers.insert("baggage", "hemmelig=verdi");

            let span = tracing::info_span!("command.validate");
            set_parent_on_span_from_nats_headers(&span, Some(&headers));
            let _guard = span.enter();

            let utgaaende = trace_headers().expect("traceparent videreføres");
            assert!(utgaaende.get("traceparent").is_some());
            assert!(utgaaende.get("baggage").is_none());
        });
    }

    #[test]
    fn uten_span_sendes_ingen_trace_headere() {
        med_otel(|| assert!(trace_headers().is_none()));
    }
}
