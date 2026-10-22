use application::admin::model::{AdminEntitetIdentitet, AdminSakFakta};
use async_nats::HeaderMap;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use domain::eksekvering::operasjon::EntitetId;
use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};

use super::*;
use crate::admin::mapping;
use crate::admin::nats::svar::{
    INTERNAL_ERROR, INVALID_REQUEST, MAX_UTFORT_AV_BYTES, RESPONSE_TOO_LARGE, valider_utfort_av,
};
use crate::nats::nats_response::NatsResponse;

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

fn read_melding(payload: Value) -> AdminMessage {
    melding("inbox.test", payload)
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
    repository: FakeReadRepository,
) -> (AdminListener, Arc<FakeReadRepository>) {
    let fakes = Fakes {
        read: Arc::new(repository),
        ..Default::default()
    };
    (fakes.listener(transport), fakes.read)
}

fn med_max_payload(max_payload: usize) -> Arc<FakeTransport> {
    Arc::new(FakeTransport {
        max_payload,
        ..Default::default()
    })
}

fn feilmelding(svar: &Value) -> String {
    svar["payload"]["message"].as_str().unwrap().to_string()
}

// ---------------------------------------------------------------------------
// Requestvalidering
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ugyldige_requester_avvises_uten_aa_kalle_use_caset() {
    let ugyldige = vec![
        raa_melding("{ikke json"),
        read_melding(json!({ "utfort_av": "test-operator" })),
        read_melding(json!({
            "utfort_av": "test-operator",
            "command_id": Uuid::new_v4(),
            "ukjent": true
        })),
        read_melding(json!({ "utfort_av": "   ", "command_id": Uuid::new_v4() })),
        read_melding(json!({ "utfort_av": "test\noperator", "command_id": Uuid::new_v4() })),
        read_melding(json!({ "utfort_av": "x".repeat(129), "command_id": Uuid::new_v4() })),
    ];
    let antall = ugyldige.len();

    let transport = Arc::new(FakeTransport::default());
    let (listener, repository) = bygg_listener(
        transport.clone(),
        FakeReadRepository::new(Svar::Funnet(command(Uuid::new_v4())), Svar::IkkeFunnet),
    );

    for ugyldig in ugyldige {
        listener
            .handle_message(AdminRute::HentCommand, ugyldig)
            .await;
    }

    let svar = transport.publiserte_svar();
    assert_eq!(svar.len(), antall);
    for svar in svar {
        assert_eq!(svar["status"], "Error");
        assert_eq!(feilmelding(&svar), INVALID_REQUEST);
    }
    assert!(
        repository.kall.lock().unwrap().is_empty(),
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
    let transport = Arc::new(FakeTransport::default());
    let (listener, _) = bygg_listener(transport.clone(), FakeReadRepository::default());

    listener
        .handle_message(
            AdminRute::HentCommand,
            read_melding(json!({ "utfort_av": "test-operator", "command_id": Uuid::new_v4() })),
        )
        .await;
    listener
        .handle_message(
            AdminRute::HentSak,
            read_melding(json!({
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
    let transport = Arc::new(FakeTransport::default());
    let (listener, _) = bygg_listener(
        transport.clone(),
        FakeReadRepository::new(Svar::Feil, Svar::Feil),
    );

    listener
        .handle_message(
            AdminRute::HentSak,
            read_melding(json!({
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
    let transport = Arc::new(FakeTransport::default());
    let sak_id = SkuffenSakId(Uuid::new_v4());
    let (listener, _) = bygg_listener(
        transport.clone(),
        FakeReadRepository::new(Svar::IkkeFunnet, Svar::Funnet(sak(sak_id, false))),
    );

    listener
        .handle_message(
            AdminRute::HentSak,
            read_melding(json!({
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

    let paa_grensen = med_max_payload(forventet.len());
    let (listener, _) = bygg_listener(
        paa_grensen.clone(),
        FakeReadRepository::new(Svar::IkkeFunnet, Svar::Funnet(sak(sak_id, true))),
    );
    listener
        .handle_message(AdminRute::HentSak, read_melding(request.clone()))
        .await;
    assert_eq!(paa_grensen.publiserte_svar()[0]["status"], "Ok");

    let over_grensen = med_max_payload(forventet.len() - 1);
    let (listener, _) = bygg_listener(
        over_grensen.clone(),
        FakeReadRepository::new(Svar::IkkeFunnet, Svar::Funnet(sak(sak_id, true))),
    );
    listener
        .handle_message(AdminRute::HentSak, read_melding(request))
        .await;
    assert_eq!(
        feilmelding(&over_grensen.publiserte_svar()[0]),
        RESPONSE_TOO_LARGE
    );
}

// ---------------------------------------------------------------------------
// Attribusjonslogg og trace context
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gyldig_request_gir_noyaktig_en_attribusjonslogg_etter_publish() {
    let fangst = LoggFangst::default();
    let _guard = fangst.installer();

    let command_id = Uuid::new_v4();
    let (listener, _) = bygg_listener(
        Arc::new(FakeTransport::default()),
        FakeReadRepository::new(Svar::Funnet(command(command_id)), Svar::IkkeFunnet),
    );

    listener
        .handle_message(
            AdminRute::HentCommand,
            read_melding(json!({ "utfort_av": "  test-operator ", "command_id": command_id })),
        )
        .await;

    let attribusjon = fangst.med_melding("admin read utført");
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
    assert_eq!(
        attribusjon[0].get("key_type").map(String::as_str),
        Some("command_id")
    );
}

#[tokio::test]
async fn publish_feil_logges_som_error_resultat() {
    let fangst = LoggFangst::default();
    let _guard = fangst.installer();

    let command_id = Uuid::new_v4();
    let (listener, _) = bygg_listener(
        Arc::new(FakeTransport {
            publish_feiler: true,
            ..Default::default()
        }),
        FakeReadRepository::new(Svar::Funnet(command(command_id)), Svar::IkkeFunnet),
    );

    listener
        .handle_message(
            AdminRute::HentCommand,
            read_melding(json!({ "utfort_av": "test-operator", "command_id": command_id })),
        )
        .await;

    let attribusjon = fangst.med_melding("admin read utført");
    assert_eq!(
        attribusjon
            .first()
            .expect("attribusjonslogg finnes")
            .get("resultat")
            .map(String::as_str),
        Some("error")
    );
}

#[tokio::test]
async fn arkiv_id_key_logges_uten_raa_verdi() {
    let fangst = LoggFangst::default();
    let _guard = fangst.installer();

    let (listener, _) = bygg_listener(
        Arc::new(FakeTransport::default()),
        FakeReadRepository::default(),
    );

    listener
        .handle_message(
            AdminRute::HentSak,
            read_melding(json!({
                "utfort_av": "test-operator",
                "key": { "type": "arkivId", "value": "2026/hemmelig" }
            })),
        )
        .await;

    let attribusjon = fangst.med_melding("admin read utført");
    let attribusjon = attribusjon.first().expect("attribusjonslogg finnes");
    assert_eq!(
        attribusjon.get("key_type").map(String::as_str),
        Some("arkiv_id")
    );
    assert_eq!(attribusjon.get("lookup").map(String::as_str), Some(""));
    assert!(
        !fangst
            .linjer
            .lock()
            .unwrap()
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
    let guard = tracing::subscriber::set_default(subscriber);

    let trace_id = "4bf92f3577b34da6a3ce929d0e0e4736";
    let traceparent = |span_id: &str| {
        let mut headers = HeaderMap::new();
        headers.insert(
            "traceparent",
            format!("00-{trace_id}-{span_id}-01").as_str(),
        );
        headers
    };

    let command_id = Uuid::new_v4();
    let (listener, _) = bygg_listener(
        Arc::new(FakeTransport::default()),
        FakeReadRepository::new(Svar::Funnet(command(command_id)), Svar::IkkeFunnet),
    );

    listener
        .handle_message(
            AdminRute::HentCommand,
            AdminMessage {
                headers: Some(traceparent("00f067aa0ba902b7")),
                ..read_melding(json!({ "utfort_av": "test-operator", "command_id": command_id }))
            },
        )
        .await;
    listener
        .handle_message(
            AdminRute::ForkastCommands,
            AdminMessage {
                headers: Some(traceparent("00f067aa0ba902b8")),
                ..read_melding(json!({ "utfort_av": "test-operator", "command_ids": [command_id] }))
            },
        )
        .await;

    drop(guard);
    provider.force_flush().unwrap();

    let spans = exporter.get_finished_spans().unwrap();
    for (navn, parent) in [
        ("admin.read", "00f067aa0ba902b7"),
        ("admin.write", "00f067aa0ba902b8"),
    ] {
        let request_span = spans
            .iter()
            .find(|span| span.name == navn)
            .unwrap_or_else(|| panic!("{navn} ble eksportert"));
        assert_eq!(request_span.span_context.trace_id().to_string(), trace_id);
        assert_eq!(request_span.parent_span_id.to_string(), parent);
    }
}
