use async_trait::async_trait;
use domain::eksekvering::typer::{CommandEvent, CommandStatus, Operasjonstatus, StatusErrorCode};
use lib_schemas::skuffen::command::commands::{
    Command as WireCommand, CommandEnvelope as WireCommandEnvelope,
};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::dokument::{Dokument, Dokumentform};
use lib_schemas::skuffen::query::queries::SakKey;
use lib_schemas::skuffen::sak::{Ordningsverdi, Saksnummer, Sakstittel};
use lib_schemas::skuffen::tilgang::Tilgjengelighet;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::command::ports::command_state_port::{
    ArkivSakTilstand, ArkivSakTilstandError, ArkivSakTilstandErrorKind, ArkivSakTilstandRepository,
};
use crate::command::ports::entitet_port::{Entitet, EntitetRepository, NyEntitet};
use crate::command::ports::status_publisher_port::StatusPublisher;
use crate::command::ports::validated_command_dispatcher_port::ValidatedCommandDispatcher;
use crate::command::services::validate_command::{ValidateCommandService, ValidationOutcome};
use crate::command::{
    Command as ApplicationCommand, CommandEnvelope as ApplicationCommandEnvelope,
};

#[derive(Clone, Default)]
struct FakeCommandStatusPublisher {
    events: Arc<Mutex<Vec<CommandStatus>>>,
}

#[async_trait]
impl StatusPublisher for FakeCommandStatusPublisher {
    async fn publiser_command_status(&self, status: CommandStatus) -> Result<(), anyhow::Error> {
        self.events.lock().unwrap().push(status);
        Ok(())
    }

    async fn publiser_operasjonstatus(
        &self,
        _status: Operasjonstatus,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeValidatedCommandDispatcher {
    dispatched: Arc<Mutex<Vec<ApplicationCommandEnvelope<ApplicationCommand>>>>,
}

#[async_trait]
impl ValidatedCommandDispatcher for FakeValidatedCommandDispatcher {
    async fn dispatch_validated(
        &self,
        command: &ApplicationCommandEnvelope<ApplicationCommand>,
    ) -> Result<(), anyhow::Error> {
        self.dispatched.lock().unwrap().push(command.clone());
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeArkivSakTilstandRepository {
    response: Arc<Mutex<ArkivSakTilstandResponse>>,
    calls: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
enum ArkivSakTilstandResponse {
    Ok(ArkivSakTilstand),
    /// Kode, melding og feilkode kommer fra adapteren i produksjon. Fake-en
    /// bærer dem uendret, så testene ser det klienten faktisk ville fått.
    Err(
        ArkivSakTilstandErrorKind,
        &'static str,
        String,
        StatusErrorCode,
    ),
}

impl Default for ArkivSakTilstandResponse {
    fn default() -> Self {
        Self::Ok(ArkivSakTilstand { avsluttet: false })
    }
}

impl FakeArkivSakTilstandRepository {
    fn set_response(&self, response: ArkivSakTilstandResponse) {
        *self.response.lock().unwrap() = response;
    }
}

#[async_trait]
impl ArkivSakTilstandRepository for FakeArkivSakTilstandRepository {
    async fn hent_sak_tilstand_fra_arkivet(
        &self,
        saksnummer: &str,
    ) -> Result<ArkivSakTilstand, ArkivSakTilstandError> {
        self.calls.lock().unwrap().push(saksnummer.to_string());
        match self.response.lock().unwrap().clone() {
            ArkivSakTilstandResponse::Ok(state) => Ok(state),
            ArkivSakTilstandResponse::Err(kind, kode, message, error_code) => {
                Err(ArkivSakTilstandError::new(kind, kode, message, error_code))
            }
        }
    }
}

#[derive(Clone)]
enum SkuffenIdResponse {
    Ok(Option<Uuid>),
    Err(String),
}

#[derive(Clone)]
enum ArkivIdResponse {
    Ok(Option<String>),
    Err(String),
}

#[derive(Clone, Default)]
struct EntitetCalls {
    write_calls: usize,
}

/// Identitetsoppslaget som `validate_sak_ref` bruker.
///
/// I v3 gir ett kall både `skuffen_id` og `arkiv_id`, så fake-en folder de to
/// tidligere responsene sammen: mangler entiteten er den `None`, og feiler
/// enten oppslaget eller arkiv-id-en propagerer den som `Err`.
#[derive(Clone)]
struct FakeEntitetRepository {
    skuffen_id: Arc<Mutex<SkuffenIdResponse>>,
    arkiv_id: Arc<Mutex<ArkivIdResponse>>,
    calls: Arc<Mutex<EntitetCalls>>,
}

impl Default for FakeEntitetRepository {
    fn default() -> Self {
        Self {
            skuffen_id: Arc::new(Mutex::new(SkuffenIdResponse::Ok(None))),
            arkiv_id: Arc::new(Mutex::new(ArkivIdResponse::Ok(None))),
            calls: Arc::new(Mutex::new(EntitetCalls::default())),
        }
    }
}

impl FakeEntitetRepository {
    fn set_skuffen_id_response(&self, response: SkuffenIdResponse) {
        *self.skuffen_id.lock().unwrap() = response;
    }

    fn set_arkiv_id_response(&self, response: ArkivIdResponse) {
        *self.arkiv_id.lock().unwrap() = response;
    }
}

#[async_trait]
impl EntitetRepository for FakeEntitetRepository {
    async fn registrer(&self, entitet: NyEntitet) -> Result<Uuid, anyhow::Error> {
        self.calls.lock().unwrap().write_calls += 1;
        Ok(entitet.skuffen_id)
    }

    async fn hent_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<Entitet>, anyhow::Error> {
        let skuffen_id = match &*self.skuffen_id.lock().unwrap() {
            SkuffenIdResponse::Err(message) => return Err(anyhow::anyhow!(message.clone())),
            SkuffenIdResponse::Ok(None) => return Ok(None),
            SkuffenIdResponse::Ok(Some(id)) => *id,
        };

        let arkiv_id = match &*self.arkiv_id.lock().unwrap() {
            ArkivIdResponse::Err(message) => return Err(anyhow::anyhow!(message.clone())),
            ArkivIdResponse::Ok(value) => value.clone(),
        };

        Ok(Some(Entitet {
            skuffen_id,
            entitet_type: domain::eksekvering::operasjon::EntitetType::Sak,
            client_reference: Some(client_reference),
            arkiv_id,
        }))
    }

    async fn hent_for_arkiv_id(
        &self,
        _entitet_type: domain::eksekvering::operasjon::EntitetType,
        _arkiv_id: &str,
    ) -> Result<Option<Entitet>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_eller_opprett_for_arkiv_id(
        &self,
        _entitet_type: domain::eksekvering::operasjon::EntitetType,
        _arkiv_id: &str,
    ) -> Result<Uuid, anyhow::Error> {
        self.calls.lock().unwrap().write_calls += 1;
        Ok(Uuid::now_v7())
    }

    async fn hent_arkiv_id(&self, _skuffen_id: Uuid) -> Result<Option<String>, anyhow::Error> {
        Ok(None)
    }
}

fn build_service(
    state_repo: FakeArkivSakTilstandRepository,
    entitet: FakeEntitetRepository,
    dispatcher: FakeValidatedCommandDispatcher,
    status_publisher: FakeCommandStatusPublisher,
) -> ValidateCommandService {
    ValidateCommandService::new(
        Box::new(state_repo),
        Box::new(entitet),
        Box::new(dispatcher),
        Box::new(status_publisher),
    )
}

fn sample_dokument() -> Dokument {
    Dokument {
        client_reference: Uuid::new_v4(),
        tittel: "Hoveddokument".to_string(),
        form: Dokumentform::Bytes {
            filtype: "PDF".to_string(),
            dokument_referanse: Uuid::new_v4(),
        },
    }
}

fn make_journalpost_command(sak_key: SakKey) -> WireCommand {
    WireCommand::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
        felles: JournalpostCommon {
            client_reference: Uuid::new_v4(),
            tittel: "Internt notat".to_string(),
            dokument_dato: "2025-01-01".to_string(),
            saksbehandler: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgjengelighet: Tilgjengelighet::Offentlig,
            dokumenter: vec![sample_dokument()],
            sak_key,
            kildesystem: None,
        },
    })
}

fn make_opprett_sak_command() -> WireCommand {
    WireCommand::OpprettSak(OpprettSak {
        client_reference: Uuid::new_v4(),
        sakstittel: Sakstittel::try_from("Test sak".to_string()).unwrap(),
        ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
        arkivdel: Arkivdel::Tilsynsdivisjonene,
        saksbehandler_id: "Z12345".to_string(),
        saksbehandler_enhet: "42".to_string(),
        tilgjengelighet: Tilgjengelighet::Offentlig,
    })
}

fn make_avslutt_sak_command(sak_key: SakKey) -> WireCommand {
    WireCommand::AvsluttSak(AvsluttSak { sak_key })
}

fn wrap_command(command: WireCommand) -> ApplicationCommandEnvelope<ApplicationCommand> {
    crate::command::test_support::map_wire_envelope(WireCommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: command,
    })
}

fn assert_statuses(
    events: &[CommandStatus],
    command_id: Uuid,
    hendelse: CommandEvent,
    expected_error_code: Option<StatusErrorCode>,
) {
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].command_id, command_id);
    assert_eq!(events[0].hendelse, hendelse);
    assert_eq!(events[0].terminal, hendelse.er_terminal());
    assert_eq!(events[0].error_code, expected_error_code);
}

/// Blokkert og recoverable er transiente: kommandoen redeliveres av NATS.
/// Vi publiserer utfall, ikke flakking (D33).
fn assert_ingen_status(events: &[CommandStatus]) {
    assert!(
        events.is_empty(),
        "transient valideringsutfall skal ikke publisere status, fikk {events:?}"
    );
}

#[tokio::test]
async fn test_validate_opprett_sak_dispatches_and_emits_ok_status() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    let service = build_service(
        state_repo.clone(),
        entitet.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let envelope = wrap_command(make_opprett_sak_command());

    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    assert!(matches!(outcome, ValidationOutcome::Ok));
    assert_eq!(dispatcher.dispatched.lock().unwrap().len(), 1);

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(&events, command_id, CommandEvent::Validert, None);

    assert!(state_repo.calls.lock().unwrap().is_empty());
    assert_eq!(entitet.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_journalpost_missing_sak_is_irrecoverable() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    let service = build_service(
        state_repo.clone(),
        entitet.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let sak_ref = Uuid::new_v4();
    let envelope = wrap_command(make_journalpost_command(SakKey::ClientReference(sak_ref)));

    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    match outcome {
        ValidationOutcome::Irrecoverable {
            message,
            error_code,
        } => {
            assert_eq!(message, "Sak finnes ikke i Skuffen");
            assert_eq!(error_code, StatusErrorCode::NotFound);
        }
        _ => panic!("Expected irrecoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandEvent::Avvist,
        Some(StatusErrorCode::NotFound),
    );

    assert!(state_repo.calls.lock().unwrap().is_empty());
    assert_eq!(entitet.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_journalpost_allows_skuffen_only_sak() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    let skuffen_id = Uuid::new_v4();
    entitet.set_skuffen_id_response(SkuffenIdResponse::Ok(Some(skuffen_id)));
    entitet.set_arkiv_id_response(ArkivIdResponse::Ok(None));

    let service = build_service(
        state_repo.clone(),
        entitet.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let sak_ref = Uuid::new_v4();
    let envelope = wrap_command(make_journalpost_command(SakKey::ClientReference(sak_ref)));

    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    assert!(matches!(outcome, ValidationOutcome::Ok));
    assert_eq!(dispatcher.dispatched.lock().unwrap().len(), 1);

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(&events, command_id, CommandEvent::Validert, None);

    assert!(state_repo.calls.lock().unwrap().is_empty());
    assert_eq!(entitet.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_journalpost_blocks_closed_sak() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    let skuffen_id = Uuid::new_v4();
    entitet.set_skuffen_id_response(SkuffenIdResponse::Ok(Some(skuffen_id)));
    entitet.set_arkiv_id_response(ArkivIdResponse::Ok(Some("2025/1".to_string())));
    state_repo.set_response(ArkivSakTilstandResponse::Ok(ArkivSakTilstand {
        avsluttet: true,
    }));

    let service = build_service(
        state_repo.clone(),
        entitet.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let sak_ref = Uuid::new_v4();
    let envelope = wrap_command(make_journalpost_command(SakKey::ClientReference(sak_ref)));

    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    match outcome {
        ValidationOutcome::Irrecoverable {
            message,
            error_code,
        } => {
            assert_eq!(message, "Sak er avsluttet");
            assert_eq!(error_code, StatusErrorCode::Conflict);
        }
        _ => panic!("Expected irrecoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandEvent::Avvist,
        Some(StatusErrorCode::Conflict),
    );

    let calls = state_repo.calls.lock().unwrap();
    assert_eq!(calls.as_slice(), ["2025/1".to_string()]);
    assert_eq!(entitet.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_arkiv_id_open_sak_is_ok() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    let service = build_service(
        state_repo.clone(),
        entitet.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let saksnummer = Saksnummer::new("2025/42").unwrap();
    let envelope = wrap_command(make_avslutt_sak_command(SakKey::ArkivId(saksnummer)));

    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    assert!(matches!(outcome, ValidationOutcome::Ok));
    assert_eq!(dispatcher.dispatched.lock().unwrap().len(), 1);

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(&events, command_id, CommandEvent::Validert, None);

    let calls = state_repo.calls.lock().unwrap();
    assert_eq!(calls.as_slice(), ["2025/42".to_string()]);
    assert_eq!(entitet.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_arkiv_id_recoverable_error_retries() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    state_repo.set_response(ArkivSakTilstandResponse::Err(
        ArkivSakTilstandErrorKind::Recoverable,
        "sikri_upstream_unavailable",
        "Sikri/Elements er midlertidig utilgjengelig. Prøv igjen senere.".to_string(),
        StatusErrorCode::TemporaryUnavailable,
    ));

    let service = build_service(
        state_repo.clone(),
        entitet.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let saksnummer = Saksnummer::new("2025/99").unwrap();
    let envelope = wrap_command(make_journalpost_command(SakKey::ArkivId(saksnummer)));

    let _command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    match outcome {
        ValidationOutcome::Recoverable {
            message,
            error_code,
        } => {
            assert_eq!(
                message,
                "Sikri/Elements er midlertidig utilgjengelig. Prøv igjen senere."
            );
            assert_eq!(error_code, StatusErrorCode::TemporaryUnavailable);
        }
        _ => panic!("Expected recoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_ingen_status(&events);
    assert_eq!(entitet.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_arkiv_id_irrecoverable_error_is_error() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    // Et ukjent saksnummer er 404 fra Sikri, som gir NotFound — ikke
    // InvalidRequest, som valideringen tidligere hardkodet for alt.
    state_repo.set_response(ArkivSakTilstandResponse::Err(
        ArkivSakTilstandErrorKind::Irrecoverable,
        "sikri_resource_not_found",
        "Fant ikke sak 2025/404 i arkivet.".to_string(),
        StatusErrorCode::NotFound,
    ));

    let service = build_service(
        state_repo.clone(),
        entitet.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let saksnummer = Saksnummer::new("2025/404").unwrap();
    let envelope = wrap_command(make_journalpost_command(SakKey::ArkivId(saksnummer)));

    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    match outcome {
        ValidationOutcome::Irrecoverable {
            message,
            error_code,
        } => {
            assert_eq!(message, "Fant ikke sak 2025/404 i arkivet.");
            assert_eq!(error_code, StatusErrorCode::NotFound);
        }
        _ => panic!("Expected irrecoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandEvent::Avvist,
        Some(StatusErrorCode::NotFound),
    );
    // Klienten får den faktiske grunnen, ikke «Forespørselen ble avvist.».
    let avvist = events
        .iter()
        .find(|event| event.hendelse == CommandEvent::Avvist)
        .expect("Avvist-event mangler");
    assert_eq!(avvist.melding, "Fant ikke sak 2025/404 i arkivet.");
    assert_eq!(entitet.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_client_reference_lookup_error_is_retrying() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    entitet.set_skuffen_id_response(SkuffenIdResponse::Err("db error".to_string()));

    let service = build_service(
        state_repo,
        entitet.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let sak_ref = Uuid::new_v4();
    let envelope = wrap_command(make_journalpost_command(SakKey::ClientReference(sak_ref)));

    let _command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    match outcome {
        ValidationOutcome::Recoverable {
            message,
            error_code,
        } => {
            assert_eq!(message, "db error");
            assert_eq!(error_code, StatusErrorCode::TemporaryUnavailable);
        }
        _ => panic!("Expected recoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_ingen_status(&events);
    assert_eq!(entitet.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_arkiv_id_lookup_error_is_retrying() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    entitet.set_skuffen_id_response(SkuffenIdResponse::Ok(Some(Uuid::new_v4())));
    entitet.set_arkiv_id_response(ArkivIdResponse::Err("lookup failed".to_string()));

    let service = build_service(
        state_repo,
        entitet.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let sak_ref = Uuid::new_v4();
    let envelope = wrap_command(make_journalpost_command(SakKey::ClientReference(sak_ref)));

    let _command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    match outcome {
        ValidationOutcome::Recoverable {
            message,
            error_code,
        } => {
            assert_eq!(message, "lookup failed");
            assert_eq!(error_code, StatusErrorCode::TemporaryUnavailable);
        }
        _ => panic!("Expected recoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_ingen_status(&events);
    assert_eq!(entitet.calls.lock().unwrap().write_calls, 0);
}
