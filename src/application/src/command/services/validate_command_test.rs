use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{
    Command, CommandEnvelope, CommandStatus, CommandStatusEvent,
};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::dokument::Dokument;
use lib_schemas::skuffen::query::queries::SakKey;
use lib_schemas::skuffen::sak::{Ordningsverdi, Sakstittel, Saksnummer};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::command::ports::command_state_port::{
    CommandStateError, CommandStateErrorKind, CommandStateRepository, SakState,
};
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::status_publisher_port::CommandStatusPublisher;
use crate::command::ports::validated_command_dispatcher_port::ValidatedCommandDispatcher;
use crate::command::services::validate_command::{ValidateCommandService, ValidationOutcome};

#[derive(Clone, Default)]
struct FakeCommandStatusPublisher {
    events: Arc<Mutex<Vec<CommandStatusEvent>>>,
}

#[async_trait]
impl CommandStatusPublisher for FakeCommandStatusPublisher {
    async fn publish_status(&self, event: CommandStatusEvent) -> Result<(), anyhow::Error> {
        self.events.lock().unwrap().push(event);
        Ok(())
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
struct FakeCommandStateRepository {
    response: Arc<Mutex<CommandStateResponse>>,
    calls: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
enum CommandStateResponse {
    Ok(SakState),
    Err(CommandStateErrorKind, String),
}

impl Default for CommandStateResponse {
    fn default() -> Self {
        Self::Ok(SakState { avsluttet: false })
    }
}

impl FakeCommandStateRepository {
    fn set_response(&self, response: CommandStateResponse) {
        *self.response.lock().unwrap() = response;
    }
}

#[async_trait]
impl CommandStateRepository for FakeCommandStateRepository {
    async fn hent_sak_state(&self, saksnummer: &str) -> Result<SakState, CommandStateError> {
        self.calls.lock().unwrap().push(saksnummer.to_string());
        match self.response.lock().unwrap().clone() {
            CommandStateResponse::Ok(state) => Ok(state),
            CommandStateResponse::Err(kind, message) => Err(CommandStateError::new(kind, message)),
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
    get_skuffen_id: usize,
    get_arkiv_id: usize,
    last_client_reference: Option<Uuid>,
    last_skuffen_id: Option<Uuid>,
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
        _skuffen_id: Uuid,
        _command: &Command,
        _arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn register_document_mapping(
        &self,
        _command_id: Uuid,
        _client_reference: Uuid,
        _skuffen_id: Uuid,
        _arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn oppdater_arkiv_id_for_client_reference(
        &self,
        _client_reference: Uuid,
        _arkiv_id: String,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn get_arkiv_id(&self, skuffen_id: Uuid) -> Result<Option<String>, anyhow::Error> {
        let mut calls = self.calls.lock().unwrap();
        calls.get_arkiv_id += 1;
        calls.last_skuffen_id = Some(skuffen_id);
        drop(calls);

        match self.responses.lock().unwrap().arkiv_id.clone() {
            ArkivIdResponse::Ok(value) => Ok(value),
            ArkivIdResponse::Err(message) => Err(anyhow::anyhow!(message)),
        }
    }

    async fn get_skuffen_id(&self, client_reference: Uuid) -> Result<Option<Uuid>, anyhow::Error> {
        let mut calls = self.calls.lock().unwrap();
        calls.get_skuffen_id += 1;
        calls.last_client_reference = Some(client_reference);
        drop(calls);

        match self.responses.lock().unwrap().skuffen_id.clone() {
            SkuffenIdResponse::Ok(value) => Ok(value),
            SkuffenIdResponse::Err(message) => Err(anyhow::anyhow!(message)),
        }
    }

    async fn get_skuffen_id_from_arkiv_id(
        &self,
        _arkiv_id: &str,
    ) -> Result<Option<Uuid>, anyhow::Error> {
        Ok(None)
    }
}

fn build_service(
    state_repo: FakeCommandStateRepository,
    id_mapping: FakeIdMappingRepository,
    dispatcher: FakeValidatedCommandDispatcher,
    status_publisher: FakeCommandStatusPublisher,
) -> ValidateCommandService {
    ValidateCommandService::new(
        Box::new(state_repo),
        Box::new(id_mapping),
        Box::new(dispatcher),
        Box::new(status_publisher),
    )
}

fn sample_dokument() -> Dokument {
    Dokument {
        client_reference: Uuid::new_v4(),
        tittel: "Hoveddokument".to_string(),
        filtype: "PDF".to_string(),
        dokument_referanse: Uuid::new_v4(),
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
    events: &[CommandStatusEvent],
    command_id: Uuid,
    final_status: CommandStatus,
    final_message: Option<&str>,
) {
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].status, CommandStatus::Pending);
    assert_eq!(events[0].command_id, command_id);
    assert!(events[0].message.is_none());
    assert_eq!(events[1].status, final_status);
    assert_eq!(events[1].command_id, command_id);
    match final_message {
        Some(expected) => assert_eq!(events[1].message.as_deref(), Some(expected)),
        None => assert!(events[1].message.is_none()),
    }
}

#[tokio::test]
async fn test_validate_opprett_sak_dispatches_and_emits_ok_status() {
    let state_repo = FakeCommandStateRepository::default();
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
    assert_statuses(&events, command_id, CommandStatus::Ok, None);

    assert!(state_repo.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_validate_journalpost_missing_sak_is_irrecoverable() {
    let state_repo = FakeCommandStateRepository::default();
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
        ValidationOutcome::Irrecoverable { message } => {
            assert_eq!(message, "Sak finnes ikke i Skuffen");
        }
        _ => panic!("Expected irrecoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Error,
        Some("Sak finnes ikke i Skuffen"),
    );

    assert!(state_repo.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_validate_journalpost_allows_skuffen_only_sak() {
    let state_repo = FakeCommandStateRepository::default();
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
    assert_statuses(&events, command_id, CommandStatus::Ok, None);

    assert!(state_repo.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn test_validate_journalpost_blocks_closed_sak() {
    let state_repo = FakeCommandStateRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    let skuffen_id = Uuid::new_v4();
    id_mapping.set_skuffen_id_response(SkuffenIdResponse::Ok(Some(skuffen_id)));
    id_mapping.set_arkiv_id_response(ArkivIdResponse::Ok(Some("2025/1".to_string())));
    state_repo.set_response(CommandStateResponse::Ok(SakState { avsluttet: true }));

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
        ValidationOutcome::Irrecoverable { message } => {
            assert_eq!(message, "Sak er avsluttet");
        }
        _ => panic!("Expected irrecoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(&events, command_id, CommandStatus::Error, Some("Sak er avsluttet"));

    let calls = state_repo.calls.lock().unwrap();
    assert_eq!(calls.as_slice(), ["2025/1".to_string()]);
}

#[tokio::test]
async fn test_validate_arkiv_id_open_sak_is_ok() {
    let state_repo = FakeCommandStateRepository::default();
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
    assert_statuses(&events, command_id, CommandStatus::Ok, None);

    let calls = state_repo.calls.lock().unwrap();
    assert_eq!(calls.as_slice(), ["2025/42".to_string()]);
}

#[tokio::test]
async fn test_validate_arkiv_id_recoverable_error_retries() {
    let state_repo = FakeCommandStateRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    state_repo.set_response(CommandStateResponse::Err(
        CommandStateErrorKind::Recoverable,
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
        ValidationOutcome::Recoverable { message } => {
            assert_eq!(message, "Sikri timeout");
        }
        _ => panic!("Expected recoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Retrying,
        Some("Sikri timeout"),
    );
}

#[tokio::test]
async fn test_validate_arkiv_id_irrecoverable_error_is_error() {
    let state_repo = FakeCommandStateRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    state_repo.set_response(CommandStateResponse::Err(
        CommandStateErrorKind::Irrecoverable,
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
        ValidationOutcome::Irrecoverable { message } => {
            assert_eq!(message, "Sak finnes ikke i Sikri (2025/404)");
        }
        _ => panic!("Expected irrecoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Error,
        Some("Sak finnes ikke i Sikri (2025/404)"),
    );
}

#[tokio::test]
async fn test_validate_client_reference_lookup_error_is_retrying() {
    let state_repo = FakeCommandStateRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    id_mapping.set_skuffen_id_response(SkuffenIdResponse::Err("db error".to_string()));

    let service = build_service(
        state_repo,
        id_mapping,
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let sak_ref = Uuid::new_v4();
    let envelope = wrap_command(make_journalpost_command(SakKey::ClientReference(sak_ref)));
    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    match outcome {
        ValidationOutcome::Recoverable { message } => {
            assert_eq!(message, "db error");
        }
        _ => panic!("Expected recoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(&events, command_id, CommandStatus::Retrying, Some("db error"));
}

#[tokio::test]
async fn test_validate_arkiv_id_lookup_error_is_retrying() {
    let state_repo = FakeCommandStateRepository::default();
    let id_mapping = FakeIdMappingRepository::default();
    let dispatcher = FakeValidatedCommandDispatcher::default();
    let status_publisher = FakeCommandStatusPublisher::default();

    id_mapping.set_skuffen_id_response(SkuffenIdResponse::Ok(Some(Uuid::new_v4())));
    id_mapping.set_arkiv_id_response(ArkivIdResponse::Err("lookup failed".to_string()));

    let service = build_service(
        state_repo,
        id_mapping,
        dispatcher.clone(),
        status_publisher.clone(),
    );

    let sak_ref = Uuid::new_v4();
    let envelope = wrap_command(make_journalpost_command(SakKey::ClientReference(sak_ref)));
    let command_id = envelope.command_id;

    let outcome = service.handle(envelope).await.unwrap();

    match outcome {
        ValidationOutcome::Recoverable { message } => {
            assert_eq!(message, "lookup failed");
        }
        _ => panic!("Expected recoverable validation outcome"),
    }

    assert!(dispatcher.dispatched.lock().unwrap().is_empty());

    let events = status_publisher.events.lock().unwrap();
    assert_statuses(
        &events,
        command_id,
        CommandStatus::Retrying,
        Some("lookup failed"),
    );
}
