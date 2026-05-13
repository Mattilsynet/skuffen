use async_trait::async_trait;
use domain::eksekvering::typer::{
    CommandLifecycleContext, CommandLifecycleEvent, CommandStage, CommandStageStatus,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope, CommandStatus};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::dokument::{Dokument, Dokumentform};
use lib_schemas::skuffen::query::queries::SakKey;
use lib_schemas::skuffen::sak::{Ordningsverdi, Saksnummer, Sakstittel};
use lib_schemas::skuffen::status::SkuffenStatusErrorCode;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::command::ports::command_state_port::{
    ArkivSakTilstand, ArkivSakTilstandError, ArkivSakTilstandErrorKind, ArkivSakTilstandRepository,
};
use crate::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::ports::status_publisher_port::CommandStatusPublisher;
use crate::command::ports::validated_command_dispatcher_port::ValidatedCommandDispatcher;
use crate::command::services::validate_command::{ValidateCommandService, ValidationOutcome};
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};

#[derive(Clone, Default)]
struct FakeCommandStatusPublisher {
    events: Arc<Mutex<Vec<CommandLifecycleEvent>>>,
}

#[async_trait]
impl CommandStatusPublisher for FakeCommandStatusPublisher {
    async fn publish_status(&self, event: CommandLifecycleEvent) -> Result<(), anyhow::Error> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeStatusContextResolver;

#[async_trait]
impl CommandOutwardStatusProjector for FakeStatusContextResolver {
    async fn resolve_context(
        &self,
        _envelope: &CommandEnvelope<Command>,
    ) -> Result<CommandLifecycleContext, anyhow::Error> {
        Ok(CommandLifecycleContext::default())
    }
}

#[derive(Clone, Default)]
struct FakeValidatedCommandDispatcher {
    dispatched: Arc<Mutex<Vec<CommandEnvelope<Command>>>>,
}

#[async_trait]
impl ValidatedCommandDispatcher for FakeValidatedCommandDispatcher {
    async fn dispatch_validated(
        &self,
        command: &CommandEnvelope<Command>,
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
    Err(ArkivSakTilstandErrorKind, String),
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
            ArkivSakTilstandResponse::Err(kind, message) => {
                Err(ArkivSakTilstandError::new(kind, message))
            }
        }
    }
}

#[derive(Clone, Default)]
struct FakeIdMappingRepository {
    responses: Arc<Mutex<IdMappingResponses>>,
    calls: Arc<Mutex<IdMappingCalls>>,
}

#[derive(Clone)]
struct IdMappingResponses {
    skuffen_id: SkuffenIdResponse,
    arkiv_id: ArkivIdResponse,
}

impl Default for IdMappingResponses {
    fn default() -> Self {
        Self {
            skuffen_id: SkuffenIdResponse::Ok(None),
            arkiv_id: ArkivIdResponse::Ok(None),
        }
    }
}

#[derive(Clone, Default)]
struct IdMappingCalls {
    hent_sak_id_fra_mapping: usize,
    hent_arkiv_id_fra_mapping: usize,
    last_client_reference: Option<Uuid>,
    last_skuffen_id: Option<SkuffenSakId>,
    write_calls: usize,
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

impl FakeIdMappingRepository {
    fn set_skuffen_id_response(&self, response: SkuffenIdResponse) {
        self.responses.lock().unwrap().skuffen_id = response;
    }

    fn set_arkiv_id_response(&self, response: ArkivIdResponse) {
        self.responses.lock().unwrap().arkiv_id = response;
    }
}

#[async_trait]
impl IdMappingRepository for FakeIdMappingRepository {
    async fn has_processed_command(&self, _command_id: Uuid) -> Result<bool, anyhow::Error> {
        Ok(false)
    }

    async fn register_mapping(
        &self,
        _command_id: Uuid,
        _client_reference: Uuid,
        _skuffen_id: SkuffenSakId,
        _command: &Command,
        _arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        self.calls.lock().unwrap().write_calls += 1;
        Ok(())
    }

    async fn register_document_mapping(
        &self,
        _command_id: Uuid,
        _client_reference: Uuid,
        _skuffen_id: SkuffenDokumentId,
        _arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        self.calls.lock().unwrap().write_calls += 1;
        Ok(())
    }

    async fn oppdater_arkiv_id_for_client_reference(
        &self,
        _client_reference: Uuid,
        _arkiv_id: String,
    ) -> Result<(), anyhow::Error> {
        self.calls.lock().unwrap().write_calls += 1;
        Ok(())
    }

    async fn hent_arkiv_id_fra_mapping(
        &self,
        skuffen_id: SkuffenSakId,
    ) -> Result<Option<String>, anyhow::Error> {
        let mut calls = self.calls.lock().unwrap();
        calls.hent_arkiv_id_fra_mapping += 1;
        calls.last_skuffen_id = Some(skuffen_id);
        drop(calls);

        match self.responses.lock().unwrap().arkiv_id.clone() {
            ArkivIdResponse::Ok(value) => Ok(value),
            ArkivIdResponse::Err(message) => Err(anyhow::anyhow!(message)),
        }
    }

    async fn hent_sak_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error> {
        let mut calls = self.calls.lock().unwrap();
        calls.hent_sak_id_fra_mapping += 1;
        calls.last_client_reference = Some(client_reference);
        drop(calls);

        match self.responses.lock().unwrap().skuffen_id.clone() {
            SkuffenIdResponse::Ok(value) => Ok(value.map(SkuffenSakId::from)),
            SkuffenIdResponse::Err(message) => Err(anyhow::anyhow!(message)),
        }
    }

    async fn hent_journalpost_id_fra_mapping(
        &self,
        _client_reference: Uuid,
    ) -> Result<Option<SkuffenJournalpostId>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_dokument_id_fra_mapping(
        &self,
        _client_reference: Uuid,
    ) -> Result<Option<SkuffenDokumentId>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_sak_id_fra_arkiv_id_i_mapping(
        &self,
        _arkiv_id: &str,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_eller_opprett_skuffen_id_for_arkiv_id(
        &self,
        _entity_type: MappingEntityType,
        _arkiv_id: &str,
    ) -> Result<SkuffenSakId, anyhow::Error> {
        self.calls.lock().unwrap().write_calls += 1;
        Ok(SkuffenSakId::from(Uuid::new_v4()))
    }

    async fn delete_arkiv_mapping(
        &self,
        _entity_type: MappingEntityType,
        _arkiv_id: &str,
    ) -> Result<(), anyhow::Error> {
        self.calls.lock().unwrap().write_calls += 1;
        Ok(())
    }
}

fn build_service(
    state_repo: FakeArkivSakTilstandRepository,
    id_mapping: FakeIdMappingRepository,
    dispatcher: FakeValidatedCommandDispatcher,
    status_publisher: FakeCommandStatusPublisher,
) -> ValidateCommandService {
    ValidateCommandService::new(
        Box::new(state_repo),
        Box::new(id_mapping),
        Box::new(dispatcher),
        Box::new(status_publisher),
        Box::new(FakeStatusContextResolver),
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

fn make_journalpost_command(sak_key: SakKey) -> Command {
    Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
        felles: JournalpostCommon {
            client_reference: Uuid::new_v4(),
            tittel: "Internt notat".to_string(),
            dokument_dato: "2025-01-01".to_string(),
            saksbehandler: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: None,
            dokumenter: vec![sample_dokument()],
            sak_key,
            kildesystem: None,
        },
    })
}

fn make_opprett_sak_command() -> Command {
    Command::OpprettSak(OpprettSak {
        client_reference: Uuid::new_v4(),
        sakstittel: Sakstittel("Test sak".to_string()),
        ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
        arkivdel: Arkivdel::Tilsynsdivisjonene,
        saksbehandler_id: "Z12345".to_string(),
        saksbehandler_enhet: "42".to_string(),
        tilgang: None,
    })
}

fn make_avslutt_sak_command(sak_key: SakKey) -> Command {
    Command::AvsluttSak(AvsluttSak { sak_key })
}

fn wrap_command(command: Command) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: command,
    }
}

fn assert_statuses(
    events: &[CommandLifecycleEvent],
    command_id: Uuid,
    final_status: CommandStatus,
    final_stage_status: CommandStageStatus,
    final_detail: Option<&str>,
    expected_error_code: Option<SkuffenStatusErrorCode>,
) {
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].status, final_status);
    assert_eq!(events[0].command_id, command_id);
    assert_eq!(events[0].stage, CommandStage::Validert);
    assert_eq!(events[0].stage_status, final_stage_status);
    match final_stage_status {
        CommandStageStatus::Ok => assert_eq!(events[0].message, "validert::ok"),
        CommandStageStatus::Blocked => assert_eq!(events[0].message, "validert::blocked"),
        CommandStageStatus::Retrying => assert_eq!(events[0].message, "validert::retrying"),
        CommandStageStatus::Error => assert_eq!(events[0].message, "validert::error"),
        CommandStageStatus::Venter => unreachable!(),
    }
    match final_detail {
        Some(expected) => assert_eq!(events[0].detail.as_deref(), Some(expected)),
        None => assert!(events[0].detail.is_none()),
    }
    assert_eq!(events[0].error_code, expected_error_code);
}

#[tokio::test]
async fn test_validate_opprett_sak_dispatches_and_emits_ok_status() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    let service = build_service(
        state_repo.clone(),
        id_mapping.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let envelope = wrap_command(make_opprett_sak_command());
    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    assert!(matches!(outcome, ValidationOutcome::Ok));
    assert_eq!(dispatcher.dispatched.lock().unwrap().len(), 1);

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Ok,
        CommandStageStatus::Ok,
        None,
        None,
    );

    assert!(state_repo.calls.lock().unwrap().is_empty());
    assert_eq!(id_mapping.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_journalpost_missing_sak_is_irrecoverable() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    let service = build_service(
        state_repo.clone(),
        id_mapping.clone(),
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
            assert_eq!(error_code, SkuffenStatusErrorCode::NotFound);
        }
        _ => panic!("Expected irrecoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Error,
        CommandStageStatus::Error,
        Some("Sak finnes ikke i Skuffen"),
        Some(SkuffenStatusErrorCode::NotFound),
    );

    assert!(state_repo.calls.lock().unwrap().is_empty());
    assert_eq!(id_mapping.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_journalpost_allows_skuffen_only_sak() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    let skuffen_id = Uuid::new_v4();
    id_mapping.set_skuffen_id_response(SkuffenIdResponse::Ok(Some(skuffen_id)));
    id_mapping.set_arkiv_id_response(ArkivIdResponse::Ok(None));

    let service = build_service(
        state_repo.clone(),
        id_mapping.clone(),
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
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Ok,
        CommandStageStatus::Ok,
        None,
        None,
    );

    assert!(state_repo.calls.lock().unwrap().is_empty());
    assert_eq!(id_mapping.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_journalpost_blocks_closed_sak() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    let skuffen_id = Uuid::new_v4();
    id_mapping.set_skuffen_id_response(SkuffenIdResponse::Ok(Some(skuffen_id)));
    id_mapping.set_arkiv_id_response(ArkivIdResponse::Ok(Some("2025/1".to_string())));
    state_repo.set_response(ArkivSakTilstandResponse::Ok(ArkivSakTilstand {
        avsluttet: true,
    }));

    let service = build_service(
        state_repo.clone(),
        id_mapping.clone(),
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
            assert_eq!(error_code, SkuffenStatusErrorCode::Conflict);
        }
        _ => panic!("Expected irrecoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Error,
        CommandStageStatus::Error,
        Some("Sak er avsluttet"),
        Some(SkuffenStatusErrorCode::Conflict),
    );

    let calls = state_repo.calls.lock().unwrap();
    assert_eq!(calls.as_slice(), ["2025/1".to_string()]);
    assert_eq!(id_mapping.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_arkiv_id_open_sak_is_ok() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    let service = build_service(
        state_repo.clone(),
        id_mapping.clone(),
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
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Ok,
        CommandStageStatus::Ok,
        None,
        None,
    );

    let calls = state_repo.calls.lock().unwrap();
    assert_eq!(calls.as_slice(), ["2025/42".to_string()]);
    assert_eq!(id_mapping.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_arkiv_id_recoverable_error_retries() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    state_repo.set_response(ArkivSakTilstandResponse::Err(
        ArkivSakTilstandErrorKind::Recoverable,
        "Sikri timeout".to_string(),
    ));

    let service = build_service(
        state_repo.clone(),
        id_mapping.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let saksnummer = Saksnummer::new("2025/99").unwrap();
    let envelope = wrap_command(make_journalpost_command(SakKey::ArkivId(saksnummer)));
    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    match outcome {
        ValidationOutcome::Recoverable {
            message,
            error_code,
        } => {
            assert_eq!(message, "Sikri timeout");
            assert_eq!(error_code, SkuffenStatusErrorCode::TemporaryUnavailable);
        }
        _ => panic!("Expected recoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Retrying,
        CommandStageStatus::Retrying,
        Some("Sikri timeout"),
        Some(SkuffenStatusErrorCode::TemporaryUnavailable),
    );
    assert_eq!(id_mapping.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_arkiv_id_irrecoverable_error_is_error() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    state_repo.set_response(ArkivSakTilstandResponse::Err(
        ArkivSakTilstandErrorKind::Irrecoverable,
        "Sak finnes ikke i Sikri (2025/404)".to_string(),
    ));

    let service = build_service(
        state_repo.clone(),
        id_mapping.clone(),
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
            assert_eq!(message, "Sak finnes ikke i Sikri (2025/404)");
            assert_eq!(error_code, SkuffenStatusErrorCode::InvalidRequest);
        }
        _ => panic!("Expected irrecoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Error,
        CommandStageStatus::Error,
        Some("Sak finnes ikke i Sikri (2025/404)"),
        Some(SkuffenStatusErrorCode::InvalidRequest),
    );
    assert_eq!(id_mapping.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_client_reference_lookup_error_is_retrying() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    id_mapping.set_skuffen_id_response(SkuffenIdResponse::Err("db error".to_string()));

    let service = build_service(
        state_repo,
        id_mapping.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let sak_ref = Uuid::new_v4();
    let envelope = wrap_command(make_journalpost_command(SakKey::ClientReference(sak_ref)));
    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    match outcome {
        ValidationOutcome::Recoverable {
            message,
            error_code,
        } => {
            assert_eq!(message, "db error");
            assert_eq!(error_code, SkuffenStatusErrorCode::TemporaryUnavailable);
        }
        _ => panic!("Expected recoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Retrying,
        CommandStageStatus::Retrying,
        Some("db error"),
        Some(SkuffenStatusErrorCode::TemporaryUnavailable),
    );
    assert_eq!(id_mapping.calls.lock().unwrap().write_calls, 0);
}

#[tokio::test]
async fn test_validate_arkiv_id_lookup_error_is_retrying() {
    let state_repo = FakeArkivSakTilstandRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    id_mapping.set_skuffen_id_response(SkuffenIdResponse::Ok(Some(Uuid::new_v4())));
    id_mapping.set_arkiv_id_response(ArkivIdResponse::Err("lookup failed".to_string()));

    let service = build_service(
        state_repo,
        id_mapping.clone(),
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let sak_ref = Uuid::new_v4();
    let envelope = wrap_command(make_journalpost_command(SakKey::ClientReference(sak_ref)));
    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    match outcome {
        ValidationOutcome::Recoverable {
            message,
            error_code,
        } => {
            assert_eq!(message, "lookup failed");
            assert_eq!(error_code, SkuffenStatusErrorCode::TemporaryUnavailable);
        }
        _ => panic!("Expected recoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Retrying,
        CommandStageStatus::Retrying,
        Some("lookup failed"),
        Some(SkuffenStatusErrorCode::TemporaryUnavailable),
    );
    assert_eq!(id_mapping.calls.lock().unwrap().write_calls, 0);
}
