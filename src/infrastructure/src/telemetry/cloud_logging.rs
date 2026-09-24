//! Loggformat for Cloud Logging.
//!
//! Skriver én JSON-linje per hendelse med feltene Cloud Logging tolker
//! spesielt, og med `logging.googleapis.com/trace` og `spanId` slik at en
//! loggpost kan åpnes direkte i Cloud Trace. Uten de feltene er logg og trace
//! to adskilte verktøy, og en operatør må gjette seg mellom dem.
//!
//! Spanfelter arves ned og skrives flatt i payloaden. Det er dette som gjør at
//! `jsonPayload.command_id = "..."` finner alt som skjedde med én melding,
//! uansett hvilket lag som logget det.

use std::io::Write;
use std::sync::OnceLock;

use opentelemetry::trace::TraceContextExt;
use serde_json::{Map, Value};
use tracing::dispatcher::{Dispatch, WeakDispatch};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

/// Reserverte nøkler Cloud Logging løfter ut av `jsonPayload`.
const TRACE_KEY: &str = "logging.googleapis.com/trace";
const SPAN_ID_KEY: &str = "logging.googleapis.com/spanId";
const TRACE_SAMPLED_KEY: &str = "logging.googleapis.com/trace_sampled";
const SOURCE_LOCATION_KEY: &str = "logging.googleapis.com/sourceLocation";

pub fn layer() -> CloudLoggingLayer {
    CloudLoggingLayer {
        make_writer: std::io::stdout,
        project_id: gcp_project_id(),
        dispatch: OnceLock::new(),
    }
}

/// Prosjekt-ID-en Cloud Logging trenger for å kvalifisere trace-navnet.
/// `GOOGLE_CLOUD_PROJECT` settes av flere Google-runtimes; den andre er
/// tjenestens egen konfigurasjon.
pub fn gcp_project_id() -> Option<String> {
    ["GOOGLE_CLOUD_PROJECT", "APP_APPLICATION__PROJECT_ID"]
        .into_iter()
        .find_map(|navn| std::env::var(navn).ok())
        .map(|verdi| verdi.trim().to_string())
        .filter(|verdi| !verdi.is_empty())
}

pub struct CloudLoggingLayer<W = fn() -> std::io::Stdout> {
    make_writer: W,
    project_id: Option<String>,
    /// Trace-oppslaget krever subscriberen selv. Den kan ikke hentes fra
    /// `dispatcher::get_default` inne i en layer-callback: `tracing` deler ut
    /// no-op-dispatcheren der, for å hindre at logging går i ring.
    dispatch: OnceLock<WeakDispatch>,
}

impl<W> CloudLoggingLayer<W> {
    #[cfg(test)]
    fn with_writer<M>(self, make_writer: M) -> CloudLoggingLayer<M> {
        CloudLoggingLayer {
            make_writer,
            project_id: self.project_id,
            dispatch: self.dispatch,
        }
    }
}

/// Feltene et span bidrar med til alle hendelser under seg.
struct Spanfelter(Map<String, Value>);

impl<S, W> Layer<S> for CloudLoggingLayer<W>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'a> MakeWriter<'a> + 'static,
{
    fn on_register_dispatch(&self, subscriber: &Dispatch) {
        let _ = self.dispatch.set(subscriber.downgrade());
    }

    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut felter = Map::new();
        attrs.record(&mut Feltsamler(&mut felter));
        span.extensions_mut().insert(Spanfelter(felter));
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        ctx: Context<'_, S>,
    ) {
        let Some(span) = ctx.span(id) else {
            return;
        };
        let mut extensions = span.extensions_mut();
        if let Some(felter) = extensions.get_mut::<Spanfelter>() {
            values.record(&mut Feltsamler(&mut felter.0));
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        // TraceKonteksten hentes først: oppslaget låner spanets extensions,
        // og må derfor ikke skje mens vi selv holder et lån.
        let trace = self.trace_kontekst(event, &ctx);

        let metadata = event.metadata();
        let mut post = Map::new();

        // Ytterste span først, slik at et nærmere span kan overstyre. Feltene
        // fra selve hendelsen vinner til slutt.
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                post.insert("span".into(), Value::String(span.name().into()));
                if let Some(felter) = span.extensions().get::<Spanfelter>() {
                    for (navn, verdi) in &felter.0 {
                        post.insert(navn.clone(), verdi.clone());
                    }
                }
            }
        }

        event.record(&mut Feltsamler(&mut post));
        post.entry("message")
            .or_insert_with(|| Value::String(metadata.name().into()));

        // Skrives etter feltene. Et felt kalt `severity` skal ikke kunne
        // omklassifisere en feil til info, og et felt kalt `time` skal ikke
        // kunne flytte hendelsen i tid.
        post.insert("time".into(), Value::String(tidsstempel()));
        post.insert("severity".into(), Value::String(severity(metadata.level())));
        post.insert("target".into(), Value::String(metadata.target().into()));

        // Cloud Loggings egne nøkler eies av formatteren alene. `tracing`
        // godtar siterte feltnavn, så `"logging.googleapis.com/trace" = …` er
        // fullt mulig å skrive. Uten dette kunne en loggpost pekt på en annen
        // trace enn den den faktisk tilhører.
        post.retain(|navn, _| !navn.starts_with("logging.googleapis.com/"));

        if let Some(trace) = trace {
            let trace_navn = match &self.project_id {
                Some(project_id) => format!("projects/{project_id}/traces/{}", trace.trace_id),
                // Cloud Logging aksepterer bare trace-id-en, men kobler den
                // ikke til Trace uten prosjektnavn.
                None => trace.trace_id,
            };
            post.insert(TRACE_KEY.into(), Value::String(trace_navn));
            post.insert(SPAN_ID_KEY.into(), Value::String(trace.span_id));
            post.insert(TRACE_SAMPLED_KEY.into(), Value::Bool(trace.sampled));
        }

        if let (Some(file), Some(line)) = (metadata.file(), metadata.line()) {
            let mut kilde = Map::new();
            kilde.insert("file".into(), Value::String(file.into()));
            kilde.insert("line".into(), Value::String(line.to_string()));
            kilde.insert("function".into(), Value::String(metadata.target().into()));
            post.insert(SOURCE_LOCATION_KEY.into(), Value::Object(kilde));
        }

        let Ok(mut linje) = serde_json::to_vec(&Value::Object(post)) else {
            return;
        };
        linje.push(b'\n');
        let _ = self.make_writer.make_writer().write_all(&linje);
    }
}

struct TraceKontekst {
    trace_id: String,
    span_id: String,
    sampled: bool,
}

impl<W> CloudLoggingLayer<W> {
    /// Leser OTel-konteksten for spanet hendelsen skjedde i.
    ///
    /// `opentelemetry::Context::current()` er tom under `tracing`, så
    /// konteksten må hentes fra spanet selv.
    fn trace_kontekst<S>(&self, event: &Event<'_>, ctx: &Context<'_, S>) -> Option<TraceKontekst>
    where
        S: Subscriber + for<'a> LookupSpan<'a>,
    {
        let span = ctx.event_span(event)?;
        let dispatch = self.dispatch.get()?.upgrade()?;
        let otel_context = tracing_opentelemetry::get_otel_context(&span.id(), &dispatch)?;
        let span_context = otel_context.span().span_context().clone();

        span_context.is_valid().then(|| TraceKontekst {
            trace_id: span_context.trace_id().to_string(),
            span_id: span_context.span_id().to_string(),
            sampled: span_context.is_sampled(),
        })
    }
}

fn tidsstempel() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
}

/// Cloud Logging kjenner ikke `TRACE`; den nærmeste er `DEBUG`.
fn severity(level: &tracing::Level) -> String {
    match *level {
        tracing::Level::ERROR => "ERROR",
        tracing::Level::WARN => "WARNING",
        tracing::Level::INFO => "INFO",
        tracing::Level::DEBUG | tracing::Level::TRACE => "DEBUG",
    }
    .to_string()
}

struct Feltsamler<'a>(&'a mut Map<String, Value>);

impl Feltsamler<'_> {
    fn sett(&mut self, field: &Field, verdi: Value) {
        self.0.insert(field.name().to_string(), verdi);
    }
}

impl Visit for Feltsamler<'_> {
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.sett(field, Value::Bool(value));
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.sett(field, Value::from(value));
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.sett(field, Value::from(value));
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        self.sett(field, Value::from(value));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.sett(field, Value::String(value.to_string()));
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.sett(field, Value::String(value.to_string()));
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.sett(field, Value::String(format!("{value:?}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::layer::SubscriberExt;

    #[derive(Clone, Default)]
    struct Oppsamler(Arc<Mutex<Vec<u8>>>);

    impl Oppsamler {
        fn linjer(&self) -> Vec<Value> {
            let bytes = self.0.lock().expect("oppsamler-lås");
            String::from_utf8_lossy(&bytes)
                .lines()
                .map(|linje| serde_json::from_str(linje).expect("gyldig JSON"))
                .collect()
        }
    }

    impl Write for Oppsamler {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("oppsamler-lås").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Oppsamler {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn logg(arbeid: impl FnOnce()) -> Vec<Value> {
        let oppsamler = Oppsamler::default();
        let subscriber =
            tracing_subscriber::Registry::default().with(layer().with_writer(oppsamler.clone()));
        tracing::subscriber::with_default(subscriber, arbeid);
        oppsamler.linjer()
    }

    #[test]
    fn hendelse_skrives_som_cloud_logging_json() {
        let linjer = logg(|| tracing::info!(command_id = "abc", "kommando mottatt"));

        let post = &linjer[0];
        assert_eq!(post["severity"], "INFO");
        assert_eq!(post["message"], "kommando mottatt");
        assert_eq!(post["command_id"], "abc");
        assert!(post["time"].as_str().is_some_and(|t| t.ends_with('Z')));
    }

    #[test]
    fn spanfelter_arves_ned_i_payloaden() {
        let linjer = logg(|| {
            let span = tracing::info_span!("command.validate", command_id = "abc");
            let _guard = span.enter();
            tracing::info!("validert");
        });

        let post = &linjer[0];
        assert_eq!(post["command_id"], "abc");
        assert_eq!(post["span"], "command.validate");
    }

    #[test]
    fn hendelsens_egne_felter_vinner_over_spanets() {
        let linjer = logg(|| {
            let span = tracing::info_span!("ytre", steg = "ytre");
            let _guard = span.enter();
            tracing::info!(steg = "hendelse", "test");
        });

        assert_eq!(linjer[0]["steg"], "hendelse");
    }

    #[test]
    fn nivaaer_oversettes_til_cloud_logging_severity() {
        let linjer = logg(|| {
            tracing::warn!("advarsel");
            tracing::error!("feil");
            tracing::debug!("detalj");
        });

        assert_eq!(linjer[0]["severity"], "WARNING");
        assert_eq!(linjer[1]["severity"], "ERROR");
        assert_eq!(linjer[2]["severity"], "DEBUG");
    }

    /// Et felt skal ikke kunne omklassifisere sin egen loggpost. Uten dette
    /// kunne `severity = "INFO"` skjult en feil for både varsling og søk.
    #[test]
    fn felter_kan_ikke_overskrive_reserverte_noekler() {
        let linjer = logg(|| {
            tracing::error!(
                severity = "INFO",
                time = "1970-01-01T00:00:00Z",
                "ekte feil"
            )
        });

        assert_eq!(linjer[0]["severity"], "ERROR");
        assert_ne!(linjer[0]["time"], "1970-01-01T00:00:00Z");
    }

    /// `tracing` godtar siterte feltnavn, så et felt kan hete nøyaktig det
    /// samme som Cloud Loggings trace-nøkkel. En loggpost skal ikke kunne
    /// peke på en annen trace enn sin egen.
    #[test]
    fn felter_kan_ikke_utgi_seg_for_cloud_logging_noekler() {
        let linjer = logg(|| {
            tracing::info!(
                "logging.googleapis.com/trace" = "projects/annet/traces/deadbeef",
                "forsøk på å forfalske trace"
            )
        });

        assert!(linjer[0].get(TRACE_KEY).is_none());
    }

    /// Én hendelse er én linje. Verdier serialiseres, aldri limes inn, så en
    /// nylinje i et felt kan ikke forfalske en ny loggpost.
    #[test]
    fn feltverdier_kan_ikke_forfalske_en_ny_loggpost() {
        let ondsinnet = "a\n{\"severity\":\"INFO\",\"message\":\"forfalsket\"}";
        let linjer = logg(|| tracing::error!(felt = ondsinnet, "ekte feil"));

        assert_eq!(linjer.len(), 1);
        assert_eq!(linjer[0]["severity"], "ERROR");
        assert_eq!(linjer[0]["felt"], ondsinnet);
    }

    #[test]
    fn uten_otel_lag_settes_ingen_trace_felter() {
        let linjer = logg(|| tracing::info!("uten trace"));

        assert!(linjer[0].get(TRACE_KEY).is_none());
        assert!(linjer[0].get(SPAN_ID_KEY).is_none());
    }

    /// Selve koblingen operatøren er ute etter: fra en loggpost i Cloud
    /// Logging til den tilhørende tracen.
    #[test]
    fn loggpost_peker_paa_tracen_den_hoerer_til() {
        use opentelemetry::trace::{TraceContextExt, TracerProvider};
        use tracing_opentelemetry::{OpenTelemetryLayer, OpenTelemetrySpanExt};

        let oppsamler = Oppsamler::default();
        let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder().build();
        let logg_layer = CloudLoggingLayer {
            make_writer: oppsamler.clone(),
            project_id: Some("mitt-prosjekt".to_string()),
            dispatch: OnceLock::new(),
        };
        let subscriber = tracing_subscriber::Registry::default()
            .with(OpenTelemetryLayer::new(provider.tracer("test")))
            .with(logg_layer);

        let mut trace_id = String::new();
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("operasjon.utfor", command_id = "abc");
            let _guard = span.enter();
            tracing::info!("operasjonsforsøk startet");
            trace_id = span.context().span().span_context().trace_id().to_string();
        });

        let post = &oppsamler.linjer()[0];
        assert_eq!(
            post[TRACE_KEY],
            format!("projects/mitt-prosjekt/traces/{trace_id}")
        );
        assert!(post[SPAN_ID_KEY].as_str().is_some_and(|id| id.len() == 16));
        assert_eq!(post["command_id"], "abc");
    }
}
