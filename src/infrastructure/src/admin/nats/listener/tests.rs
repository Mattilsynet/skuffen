use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use application::admin::model::{
    AdminCommand, AdminEntitetIdentitet, AdminSak, AdminSakFakta, AdminSakNokkel,
};
use application::admin::ports::admin_read_repository::AdminReadRepository;
use chrono::{DateTime, Utc};
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::operasjon::EntitetId;
use futures::stream;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};
use serde_json::json;
use tracing_subscriber::layer::SubscriberExt;

use super::*;

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum Svar<T> {
    Funnet(T),
    IkkeFunnet,
    Feil,
}

#[derive(Clone)]
struct FakeRepository {
    command: Svar<AdminCommand>,
    sak: Svar<AdminSak>,
    kall: Arc<Mutex<usize>>,
}

impl FakeRepository {
    fn new(command: Svar<AdminCommand>, sak: Svar<AdminSak>) -> Self {
        Self {
            command,
            sak,
            kall: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait]
impl AdminReadRepository for FakeRepository {
    async fn hent_command(&self, _command_id: Uuid) -> Result<Option<AdminCommand>, anyhow::Error> {
        *self.kall.lock().unwrap() += 1;
        match self.command.clone() {
            Svar::Funnet(command) => Ok(Some(command)),
            Svar::IkkeFunnet => Ok(None),
            Svar::Feil => Err(anyhow::anyhow!("db nede")),
        }
    }

    async fn hent_sak(&self, _key: AdminSakNokkel) -> Result<Option<AdminSak>, anyhow::Error> {
        *self.kall.lock().unwrap() += 1;
        match self.sak.clone() {
            Svar::Funnet(sak) => Ok(Some(sak)),
            Svar::IkkeFunnet => Ok(None),
            Svar::Feil => Err(anyhow::anyhow!("db nede")),
        }
    }
}

#[derive(Default)]
struct FakeTransport {
    meldinger: Mutex<HashMap<&'static str, Vec<AdminMessage>>>,
    /// Subjects som aldri gir meldinger og aldri avsluttes.
    uendelige: Mutex<Vec<&'static str>>,
    publisert: Mutex<Vec<(String, Vec<u8>)>>,
    max_payload: usize,
    publish_feiler: bool,
}

impl FakeTransport {
    fn ny(max_payload: usize) -> Self {
        Self {
            max_payload,
            ..Default::default()
        }
    }

    fn med_publish_feil() -> Self {
        Self {
            max_payload: usize::MAX,
            publish_feiler: true,
            ..Default::default()
        }
    }

    fn med_avsluttet_subscription(subject: &'static str, uendelig: &'static str) -> Self {
        let transport = Self::ny(usize::MAX);
        transport
            .meldinger
            .lock()
            .unwrap()
            .insert(subject, Vec::new());
        transport.uendelige.lock().unwrap().push(uendelig);
        transport
    }

    fn publiserte_svar(&self) -> Vec<serde_json::Value> {
        self.publisert
            .lock()
            .unwrap()
            .iter()
            .map(|(_, payload)| serde_json::from_slice(payload).unwrap())
            .collect()
    }
}

#[async_trait]
impl AdminTransport for FakeTransport {
    async fn queue_subscribe(
        &self,
        subject: &'static str,
        _queue_group: &'static str,
    ) -> anyhow::Result<AdminMessageStream> {
        if self.uendelige.lock().unwrap().contains(&subject) {
            return Ok(Box::pin(stream::pending()));
        }
        let meldinger = self
            .meldinger
            .lock()
            .unwrap()
            .remove(subject)
            .unwrap_or_default();
        Ok(Box::pin(stream::iter(meldinger)))
    }

    async fn publish(&self, reply: String, payload: Vec<u8>) -> anyhow::Result<()> {
        if self.publish_feiler {
            return Err(anyhow::anyhow!("publish feilet"));
        }
        self.publisert.lock().unwrap().push((reply, payload));
        Ok(())
    }

    fn max_payload(&self) -> usize {
        self.max_payload
    }
}

// ---------------------------------------------------------------------------
// Hjelpere
// ---------------------------------------------------------------------------

fn tidspunkt() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-27T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn command(command_id: Uuid) -> AdminCommand {
    AdminCommand {
        command_id,
        correlation_id: None,
        command_type: "opprett_sak".to_string(),
        mottatt_at: tidspunkt(),
        dispatchet_at: None,
        dekomponert_at: None,
        operasjoner: Vec::new(),
    }
}

fn sak(sak_id: SkuffenSakId, med_fakta: bool) -> AdminSak {
    AdminSak {
        identitet: AdminEntitetIdentitet {
            skuffen_id: EntitetId::Sak(sak_id),
            client_reference: None,
            arkiv_id: None,
            created_at: tidspunkt(),
            updated_at: tidspunkt(),
        },
        fakta: med_fakta.then(|| AdminSakFakta {
            tilstand: "opprettet".to_string(),
            sakstittel: None,
            arkivdel: None,
            ordningsverdi: None,
            opprettelse_saksbehandler_id: None,
            opprettelse_saksbehandler_enhet: None,
            tilgangskode: None,
            tilgangshjemmel: None,
            oensket_saksansvarlig_id: None,
            oensket_saksansvarlig_enhet: None,
            naavaerende_saksansvarlig_id: None,
            naavaerende_saksansvarlig_enhet: None,
            opprettet_av_command_id: Uuid::new_v4(),
            created_at: tidspunkt(),
            updated_at: tidspunkt(),
            journalposter: Vec::new(),
        }),
        operasjoner: Vec::new(),
    }
}

fn melding(payload: serde_json::Value) -> AdminMessage {
    AdminMessage {
        reply: Some("inbox.test".to_string()),
        headers: None,
        payload: Bytes::from(serde_json::to_vec(&payload).unwrap()),
    }
}

fn raa_melding(payload: &str) -> AdminMessage {
    AdminMessage {
        reply: Some("inbox.test".to_string()),
        headers: None,
        payload: Bytes::from(payload.to_string()),
    }
}

fn bygg_listener(
    transport: Arc<FakeTransport>,
    repository: Arc<FakeRepository>,
) -> (AdminListener, Arc<FakeRepository>) {
    let service = Arc::new(AdminReadService::new(repository.clone()));
    (
        AdminListener::new(transport, service, CancellationToken::new()),
        repository,
    )
}

fn feilmelding(svar: &serde_json::Value) -> String {
    svar["payload"]["message"].as_str().unwrap().to_string()
}

// ---------------------------------------------------------------------------
// Requestvalidering
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ugyldige_requester_avvises_uten_aa_kalle_use_caset() {
    let ugyldige = vec![
        raa_melding("{ikke json"),
        melding(json!({ "utfort_av": "test-operator" })),
        melding(json!({
            "utfort_av": "test-operator",
            "command_id": Uuid::new_v4(),
            "ukjent": true
        })),
        melding(json!({ "utfort_av": "   ", "command_id": Uuid::new_v4() })),
        melding(json!({ "utfort_av": "test\noperator", "command_id": Uuid::new_v4() })),
        melding(json!({ "utfort_av": "x".repeat(129), "command_id": Uuid::new_v4() })),
    ];

    let transport = Arc::new(FakeTransport::ny(usize::MAX));
    let repository = Arc::new(FakeRepository::new(
        Svar::Funnet(command(Uuid::new_v4())),
        Svar::IkkeFunnet,
    ));
    let (listener, repository) = bygg_listener(transport.clone(), repository);

    for ugyldig in ugyldige {
        listener
            .handle_message(AdminAction::HentCommand, ugyldig)
            .await;
    }

    for svar in transport.publiserte_svar() {
        assert_eq!(svar["status"], "Error");
        assert_eq!(feilmelding(&svar), INVALID_REQUEST);
    }
    assert_eq!(
        *repository.kall.lock().unwrap(),
        0,
        "use caset skal ikke kalles for ugyldige requester"
    );
}

#[tokio::test]
async fn utfort_av_trimmes_og_grensen_er_eksakt() {
    assert_eq!(
        valider_utfort_av("  test-operator  ").ok().as_deref(),
        Some("test-operator")
    );
    assert!(valider_utfort_av(&"x".repeat(MAX_UTFORT_AV_BYTES)).is_ok());
    assert!(valider_utfort_av(&"x".repeat(MAX_UTFORT_AV_BYTES + 1)).is_err());
    assert!(valider_utfort_av("").is_err());
    assert!(valider_utfort_av("\t").is_err());
    assert!(valider_utfort_av("a\u{0007}b").is_err());
}

// ---------------------------------------------------------------------------
// Feilkontrakten
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ukjent_command_og_sak_gir_stabile_not_found_meldinger() {
    let transport = Arc::new(FakeTransport::ny(usize::MAX));
    let (listener, _) = bygg_listener(
        transport.clone(),
        Arc::new(FakeRepository::new(Svar::IkkeFunnet, Svar::IkkeFunnet)),
    );

    listener
        .handle_message(
            AdminAction::HentCommand,
            melding(json!({ "utfort_av": "test-operator", "command_id": Uuid::new_v4() })),
        )
        .await;
    listener
        .handle_message(
            AdminAction::HentSak,
            melding(json!({
                "utfort_av": "test-operator",
                "key": { "type": "arkivId", "value": "2026/12345" }
            })),
        )
        .await;

    let svar = transport.publiserte_svar();
    assert_eq!(feilmelding(&svar[0]), COMMAND_NOT_FOUND);
    assert_eq!(feilmelding(&svar[1]), SAK_NOT_FOUND);
}

#[tokio::test]
async fn repositoryfeil_gir_internal_error_uten_detaljer() {
    let transport = Arc::new(FakeTransport::ny(usize::MAX));
    let (listener, _) = bygg_listener(
        transport.clone(),
        Arc::new(FakeRepository::new(Svar::Feil, Svar::Feil)),
    );

    listener
        .handle_message(
            AdminAction::HentSak,
            melding(json!({
                "utfort_av": "test-operator",
                "key": { "type": "clientReference", "value": Uuid::new_v4() }
            })),
        )
        .await;

    let svar = transport.publiserte_svar();
    assert_eq!(feilmelding(&svar[0]), INTERNAL_ERROR);
}

#[tokio::test]
async fn identity_only_sak_besvares_som_success() {
    let transport = Arc::new(FakeTransport::ny(usize::MAX));
    let sak_id = SkuffenSakId(Uuid::new_v4());
    let (listener, _) = bygg_listener(
        transport.clone(),
        Arc::new(FakeRepository::new(
            Svar::IkkeFunnet,
            Svar::Funnet(sak(sak_id, false)),
        )),
    );

    listener
        .handle_message(
            AdminAction::HentSak,
            melding(json!({
                "utfort_av": "test-operator",
                "key": { "type": "skuffenId", "value": sak_id.0 }
            })),
        )
        .await;

    let svar = transport.publiserte_svar();
    assert_eq!(svar[0]["status"], "Ok");
    assert!(svar[0]["payload"]["fakta"].is_null());
}

// ---------------------------------------------------------------------------
// Størrelsesguard
// ---------------------------------------------------------------------------

#[tokio::test]
async fn size_guard_maaler_hele_ok_svaret_og_tillater_eksakt_grense() {
    let sak_id = SkuffenSakId(Uuid::new_v4());
    let forventet = serde_json::to_vec(&NatsResponse::Ok(mapping::til_sak_response(sak(
        sak_id, true,
    ))))
    .unwrap();

    let request = json!({
        "utfort_av": "test-operator",
        "key": { "type": "skuffenId", "value": sak_id.0 }
    });

    let paa_grensen = Arc::new(FakeTransport::ny(forventet.len()));
    let (listener, _) = bygg_listener(
        paa_grensen.clone(),
        Arc::new(FakeRepository::new(
            Svar::IkkeFunnet,
            Svar::Funnet(sak(sak_id, true)),
        )),
    );
    listener
        .handle_message(AdminAction::HentSak, melding(request.clone()))
        .await;
    assert_eq!(paa_grensen.publiserte_svar()[0]["status"], "Ok");

    let over_grensen = Arc::new(FakeTransport::ny(forventet.len() - 1));
    let (listener, _) = bygg_listener(
        over_grensen.clone(),
        Arc::new(FakeRepository::new(
            Svar::IkkeFunnet,
            Svar::Funnet(sak(sak_id, true)),
        )),
    );
    listener
        .handle_message(AdminAction::HentSak, melding(request))
        .await;
    assert_eq!(
        feilmelding(&over_grensen.publiserte_svar()[0]),
        RESPONSE_TOO_LARGE
    );
}

// ---------------------------------------------------------------------------
// Subscriptions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn avsluttet_command_subscription_avslutter_run_once() {
    let transport = Arc::new(FakeTransport::med_avsluttet_subscription(
        ADMIN_READ_COMMAND_HENT_SUBJECT,
        ADMIN_READ_SAK_HENT_SUBJECT,
    ));
    let (listener, _) = bygg_listener(
        transport,
        Arc::new(FakeRepository::new(Svar::IkkeFunnet, Svar::IkkeFunnet)),
    );

    let feil = listener.run_once().await.unwrap_err();

    assert!(feil.to_string().contains(ADMIN_READ_COMMAND_HENT_SUBJECT));
}

#[tokio::test]
async fn avsluttet_sak_subscription_avslutter_run_once() {
    let transport = Arc::new(FakeTransport::med_avsluttet_subscription(
        ADMIN_READ_SAK_HENT_SUBJECT,
        ADMIN_READ_COMMAND_HENT_SUBJECT,
    ));
    let (listener, _) = bygg_listener(
        transport,
        Arc::new(FakeRepository::new(Svar::IkkeFunnet, Svar::IkkeFunnet)),
    );

    let feil = listener.run_once().await.unwrap_err();

    assert!(feil.to_string().contains(ADMIN_READ_SAK_HENT_SUBJECT));
}

#[tokio::test]
async fn shutdown_avslutter_subscriptions_uten_feil() {
    let transport = Arc::new(FakeTransport::ny(usize::MAX));
    transport
        .uendelige
        .lock()
        .unwrap()
        .extend([ADMIN_READ_COMMAND_HENT_SUBJECT, ADMIN_READ_SAK_HENT_SUBJECT]);
    let shutdown = CancellationToken::new();
    let service = Arc::new(AdminReadService::new(Arc::new(FakeRepository::new(
        Svar::IkkeFunnet,
        Svar::IkkeFunnet,
    ))));
    let listener = AdminListener::new(transport, service, shutdown.clone());

    shutdown.cancel();

    listener.run_once().await.expect("shutdown er normal slutt");
}

// ---------------------------------------------------------------------------
// Attribusjonslogg og trace context
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct LoggFangst {
    linjer: Arc<Mutex<Vec<HashMap<String, String>>>>,
}

struct LoggLag {
    fangst: LoggFangst,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LoggLag {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut felter = HashMap::new();
        event.record(&mut FeltSamler(&mut felter));
        self.fangst.linjer.lock().unwrap().push(felter);
    }
}

struct FeltSamler<'a>(&'a mut HashMap<String, String>);

impl tracing::field::Visit for FeltSamler<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().to_string(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
}

#[tokio::test]
async fn gyldig_request_gir_noyaktig_en_attribusjonslogg_etter_publish() {
    let fangst = LoggFangst::default();
    let subscriber = tracing_subscriber::registry().with(LoggLag {
        fangst: fangst.clone(),
    });
    let _guard = tracing::subscriber::set_default(subscriber);

    let transport = Arc::new(FakeTransport::ny(usize::MAX));
    let command_id = Uuid::new_v4();
    let (listener, _) = bygg_listener(
        transport,
        Arc::new(FakeRepository::new(
            Svar::Funnet(command(command_id)),
            Svar::IkkeFunnet,
        )),
    );

    listener
        .handle_message(
            AdminAction::HentCommand,
            melding(json!({ "utfort_av": "  test-operator ", "command_id": command_id })),
        )
        .await;

    let linjer = fangst.linjer.lock().unwrap();
    let attribusjon: Vec<_> = linjer
        .iter()
        .filter(|linje| {
            linje
                .get("message")
                .is_some_and(|m| m == "admin read utført")
        })
        .collect();

    assert_eq!(attribusjon.len(), 1);
    assert_eq!(
        attribusjon[0].get("utfort_av").map(String::as_str),
        Some("test-operator")
    );
    assert_eq!(
        attribusjon[0].get("resultat").map(String::as_str),
        Some("ok")
    );
    assert_eq!(
        attribusjon[0].get("lookup").map(String::as_str),
        Some(command_id.to_string().as_str())
    );
}

#[tokio::test]
async fn publish_feil_logges_som_error_resultat() {
    let fangst = LoggFangst::default();
    let subscriber = tracing_subscriber::registry().with(LoggLag {
        fangst: fangst.clone(),
    });
    let _guard = tracing::subscriber::set_default(subscriber);

    let command_id = Uuid::new_v4();
    let (listener, _) = bygg_listener(
        Arc::new(FakeTransport::med_publish_feil()),
        Arc::new(FakeRepository::new(
            Svar::Funnet(command(command_id)),
            Svar::IkkeFunnet,
        )),
    );

    listener
        .handle_message(
            AdminAction::HentCommand,
            melding(json!({ "utfort_av": "test-operator", "command_id": command_id })),
        )
        .await;

    let linjer = fangst.linjer.lock().unwrap();
    let attribusjon = linjer
        .iter()
        .find(|linje| {
            linje
                .get("message")
                .is_some_and(|m| m == "admin read utført")
        })
        .expect("attribusjonslogg finnes");
    assert_eq!(
        attribusjon.get("resultat").map(String::as_str),
        Some("error")
    );
}

#[tokio::test]
async fn arkiv_id_key_logges_uten_raa_verdi() {
    let fangst = LoggFangst::default();
    let subscriber = tracing_subscriber::registry().with(LoggLag {
        fangst: fangst.clone(),
    });
    let _guard = tracing::subscriber::set_default(subscriber);

    let transport = Arc::new(FakeTransport::ny(usize::MAX));
    let (listener, _) = bygg_listener(
        transport,
        Arc::new(FakeRepository::new(Svar::IkkeFunnet, Svar::IkkeFunnet)),
    );

    listener
        .handle_message(
            AdminAction::HentSak,
            melding(json!({
                "utfort_av": "test-operator",
                "key": { "type": "arkivId", "value": "2026/hemmelig" }
            })),
        )
        .await;

    let linjer = fangst.linjer.lock().unwrap();
    let attribusjon = linjer
        .iter()
        .find(|linje| {
            linje
                .get("message")
                .is_some_and(|m| m == "admin read utført")
        })
        .expect("attribusjonslogg finnes");
    assert_eq!(
        attribusjon.get("key_type").map(String::as_str),
        Some("arkiv_id")
    );
    assert_eq!(attribusjon.get("lookup").map(String::as_str), Some(""));
    assert!(
        !linjer
            .iter()
            .any(|linje| linje.values().any(|verdi| verdi.contains("hemmelig"))),
        "rå arkiv-id skal ikke logges"
    );
}

#[tokio::test]
async fn traceparent_blir_parent_paa_request_spanet() {
    global::set_text_map_propagator(TraceContextPropagator::new());
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(provider.tracer("admin-test")));
    let _guard = tracing::subscriber::set_default(subscriber);

    let trace_id = "4bf92f3577b34da6a3ce929d0e0e4736";
    let mut headers = HeaderMap::new();
    headers.insert(
        "traceparent",
        format!("00-{trace_id}-00f067aa0ba902b7-01").as_str(),
    );

    let transport = Arc::new(FakeTransport::ny(usize::MAX));
    let command_id = Uuid::new_v4();
    let (listener, _) = bygg_listener(
        transport,
        Arc::new(FakeRepository::new(
            Svar::Funnet(command(command_id)),
            Svar::IkkeFunnet,
        )),
    );

    listener
        .handle_message(
            AdminAction::HentCommand,
            AdminMessage {
                reply: Some("inbox.test".to_string()),
                headers: Some(headers),
                payload: Bytes::from(
                    serde_json::to_vec(
                        &json!({ "utfort_av": "test-operator", "command_id": command_id }),
                    )
                    .unwrap(),
                ),
            },
        )
        .await;

    drop(_guard);
    provider.force_flush().unwrap();

    let spans = exporter.get_finished_spans().unwrap();
    let request_span = spans
        .iter()
        .find(|span| span.name == "admin.read")
        .expect("request-spanet ble eksportert");
    assert_eq!(request_span.span_context.trace_id().to_string(), trace_id);
    assert_eq!(request_span.parent_span_id.to_string(), "00f067aa0ba902b7");
}
