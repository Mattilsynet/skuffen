use crate::command::ports::command_dispatcher_port::CommandDispatcher;
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::services::ingest_command::IngestCommandService;
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope, CommandSequence};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInngåendeJournalpost, OpprettInterntNotatJournalpost,
    OpprettUgåendeJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, OpprettSak};

use lib_schemas::skuffen::query::queries::SakKey;
use lib_schemas::skuffen::sak::{Ordningsverdi, Sakstittel};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// --- Fakes ---

#[derive(Clone, Default)]
struct FakeIdMappingRepository {
    // (command_id, client_reference, skuffen_id, entity_type, arkiv_id)
    pub mappings: Arc<Mutex<Vec<(Uuid, Uuid, Uuid, String, Option<String>)>>>,
    pub should_fail: bool,
}

#[async_trait]
impl IdMappingRepository for FakeIdMappingRepository {
    async fn has_processed_command(&self, command_id: Uuid) -> Result<bool, anyhow::Error> {
        let store = self.mappings.lock().unwrap();
        Ok(store.iter().any(|(cid, _, _, _, _)| *cid == command_id))
    }

    async fn register_mapping(
        &self,
        command_id: Uuid,
        client_reference: Uuid,
        skuffen_id: Uuid,
        entity_type: String,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        if self.should_fail {
            return Err(anyhow::anyhow!("DB Error"));
        }
        let mut store = self.mappings.lock().unwrap();
        store.push((
            command_id,
            client_reference,
            skuffen_id,
            entity_type,
            arkiv_id,
        ));
        Ok(())
    }

    async fn get_arkiv_id(&self, _skuffen_id: Uuid) -> Result<Option<String>, anyhow::Error> {
        Ok(None)
    }

    async fn get_skuffen_id(&self, _client_reference: Uuid) -> Result<Option<Uuid>, anyhow::Error> {
        Ok(None)
    }

    async fn get_skuffen_id_from_arkiv_id(
        &self,
        _arkiv_id: &str,
    ) -> Result<Option<Uuid>, anyhow::Error> {
        Ok(None)
    }
}

#[derive(Clone, Default)]
struct FakeCommandDispatcher {
    pub dispatched: Arc<Mutex<Vec<CommandEnvelope<Command>>>>,
    pub should_fail: bool,
}

#[async_trait]
impl CommandDispatcher for FakeCommandDispatcher {
    async fn dispatch(&self, command: &CommandEnvelope<Command>) -> Result<(), anyhow::Error> {
        if self.should_fail {
            return Err(anyhow::anyhow!("NATS Error"));
        }
        let mut store = self.dispatched.lock().unwrap();
        store.push(command.clone());
        Ok(())
    }
}

// --- Tests ---

#[tokio::test]
async fn test_ingest_command_opprett_sak_success() {
    // Arrange
    let fake_mapping = FakeIdMappingRepository::default();
    let fake_dispatcher = FakeCommandDispatcher::default();

    let command_id = Uuid::new_v4();
    let command = Command::OpprettSak(OpprettSak {
        client_reference: Uuid::new_v4(),
        sakstittel: Sakstittel("Test Sak".to_string()),
        ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
        arkivdel: Arkivdel::Tilsynsdivisjonene,
        saksbehandler_id: "Z99999".to_string(),
        saksbehandler_enhet: "42".to_string(),
        tilgang: None,
    });
    let envelope = CommandEnvelope {
        command_id,
        correlation_id: Some(Uuid::new_v4()),
        payload: command,
    };
    let sequence = CommandSequence::try_from(vec![envelope]).unwrap();

    let service = IngestCommandService::new(
        Box::new(fake_mapping.clone()),
        Box::new(fake_dispatcher.clone()),
    );

    // Act
    let result: anyhow::Result<()> = service.handle(sequence).await;

    // Assert
    assert!(result.is_ok());

    // Verify Mapping - Should be present for OpprettSak now
    let mappings = fake_mapping.mappings.lock().unwrap();
    assert_eq!(
        mappings.len(),
        1,
        "OpprettSak SHOULD register mapping again"
    );
    assert_eq!(mappings[0].0, command_id);
    assert_eq!(mappings[0].3, "sak");

    // Verify Dispatch
    let dispatched = fake_dispatcher.dispatched.lock().unwrap();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].command_id, command_id);
}

#[tokio::test]
async fn test_ingest_command_journalpost_success() {
    // Arrange
    let fake_mapping = FakeIdMappingRepository::default();
    let fake_dispatcher = FakeCommandDispatcher::default();

    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    let command = Command::OpprettInngåendeJournalpost(OpprettInngåendeJournalpost {
        felles: JournalpostCommon {
            client_reference,
            tittel: "Inngående brev".to_string(),
            dokument_dato: "2023-01-01".to_string(),
            saksbehandler: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: None,
            dokumenter: vec![],
            sak_key: SakKey::ClientReference(client_reference),
            kildesystem: None,
        },
        avsender: "Avsender AS".to_string(),
        mottaker: None,
    });

    let envelope = CommandEnvelope {
        command_id,
        correlation_id: Some(Uuid::new_v4()),
        payload: command,
    };
    let sequence = CommandSequence::try_from(vec![envelope]).unwrap();

    let service = IngestCommandService::new(
        Box::new(fake_mapping.clone()),
        Box::new(fake_dispatcher.clone()),
    );

    // Act
    let result: anyhow::Result<()> = service.handle(sequence).await;

    // Assert
    assert!(result.is_ok());

    // Verify Mapping
    let mappings = fake_mapping.mappings.lock().unwrap();
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].0, command_id);
    assert_eq!(mappings[0].3, "journalpost");

    // Verify Dispatch
    let dispatched = fake_dispatcher.dispatched.lock().unwrap();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].command_id, command_id);
}

#[tokio::test]
async fn test_ingest_command_idempotency_duplicate_command() {
    // Arrange
    let fake_mapping = FakeIdMappingRepository::default();
    let fake_dispatcher = FakeCommandDispatcher::default();

    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    let command = Command::OpprettInngåendeJournalpost(OpprettInngåendeJournalpost {
        felles: JournalpostCommon {
            client_reference,
            tittel: "Inngående brev".to_string(),
            dokument_dato: "2023-01-01".to_string(),
            saksbehandler: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: None,
            dokumenter: vec![],
            sak_key: SakKey::ClientReference(client_reference),
            kildesystem: None,
        },
        avsender: "Avsender AS".to_string(),
        mottaker: None,
    });

    let envelope = CommandEnvelope {
        command_id,
        correlation_id: Some(Uuid::new_v4()),
        payload: command,
    };
    let sequence1 = CommandSequence::try_from(vec![envelope.clone()]).unwrap();
    let sequence2 = CommandSequence::try_from(vec![envelope.clone()]).unwrap();

    let service = IngestCommandService::new(
        Box::new(fake_mapping.clone()),
        Box::new(fake_dispatcher.clone()),
    );

    // Act - First Call
    let result1 = service.handle(sequence1).await;
    assert!(result1.is_ok());

    // Act - Second Call (Duplicate)
    let result2 = service.handle(sequence2).await;
    assert!(result2.is_ok());

    // Assert
    let mappings = fake_mapping.mappings.lock().unwrap();
    assert_eq!(mappings.len(), 1, "Should only register mapping once");

    let dispatched = fake_dispatcher.dispatched.lock().unwrap();
    assert_eq!(
        dispatched.len(),
        1,
        "Should only dispatch once if idempotent"
    );
}

#[tokio::test]
async fn test_ingest_command_mapping_failure() {
    // Arrange
    let mut fake_mapping = FakeIdMappingRepository::default();
    fake_mapping.should_fail = true;

    let fake_dispatcher = FakeCommandDispatcher::default();

    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    let command = Command::OpprettInngåendeJournalpost(OpprettInngåendeJournalpost {
        felles: JournalpostCommon {
            client_reference,
            tittel: "Inngående brev".to_string(),
            dokument_dato: "2023-01-01".to_string(),
            saksbehandler: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: None,
            dokumenter: vec![],
            sak_key: SakKey::ClientReference(client_reference),
            kildesystem: None,
        },
        avsender: "Avsender AS".to_string(),
        mottaker: None,
    });

    let envelope = CommandEnvelope {
        command_id,
        correlation_id: Some(Uuid::new_v4()),
        payload: command,
    };
    let sequence = CommandSequence::try_from(vec![envelope]).unwrap();

    let service = IngestCommandService::new(
        Box::new(fake_mapping.clone()),
        Box::new(fake_dispatcher.clone()),
    );

    // Act
    let result: anyhow::Result<()> = service.handle(sequence).await;

    // Assert
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "Failed to register id_mapping"
    );

    // Verify NO dispatch
    let dispatched = fake_dispatcher.dispatched.lock().unwrap();
    assert_eq!(dispatched.len(), 0);
}

#[tokio::test]
async fn test_ingest_command_dispatch_failure() {
    // Arrange
    let fake_mapping = FakeIdMappingRepository::default();
    let mut fake_dispatcher = FakeCommandDispatcher::default();
    fake_dispatcher.should_fail = true;

    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    let command = Command::OpprettInngåendeJournalpost(OpprettInngåendeJournalpost {
        felles: JournalpostCommon {
            client_reference,
            tittel: "Inngående brev".to_string(),
            dokument_dato: "2023-01-01".to_string(),
            saksbehandler: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: None,
            dokumenter: vec![],
            sak_key: SakKey::ClientReference(client_reference),
            kildesystem: None,
        },
        avsender: "Avsender AS".to_string(),
        mottaker: None,
    });

    let envelope = CommandEnvelope {
        command_id,
        correlation_id: Some(Uuid::new_v4()),
        payload: command,
    };
    let sequence = CommandSequence::try_from(vec![envelope]).unwrap();

    let service = IngestCommandService::new(
        Box::new(fake_mapping.clone()),
        Box::new(fake_dispatcher.clone()),
    );

    // Act
    let result: anyhow::Result<()> = service.handle(sequence).await;

    // Assert
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "Failed to dispatch command"
    );

    // But mapping should have happened
    let mappings = fake_mapping.mappings.lock().unwrap();
    assert_eq!(mappings.len(), 1);
}

#[tokio::test]
async fn test_ingest_command_utgående_journalpost_success() {
    // Arrange
    let fake_mapping = FakeIdMappingRepository::default();
    let fake_dispatcher = FakeCommandDispatcher::default();

    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    let command = Command::OpprettUtgåendeJournalpost(OpprettUgåendeJournalpost {
        felles: JournalpostCommon {
            client_reference,
            tittel: "Utgående brev".to_string(),
            dokument_dato: "2023-01-02".to_string(),
            saksbehandler: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: None,
            dokumenter: vec![],
            sak_key: SakKey::ClientReference(client_reference),
            kildesystem: None,
        },
        avsender: None,
        mottaker: "Mottaker AS".to_string(),
    });

    let envelope = CommandEnvelope {
        command_id,
        correlation_id: Some(Uuid::new_v4()),
        payload: command,
    };
    let sequence = CommandSequence::try_from(vec![envelope]).unwrap();

    let service = IngestCommandService::new(
        Box::new(fake_mapping.clone()),
        Box::new(fake_dispatcher.clone()),
    );

    // Act
    let result: anyhow::Result<()> = service.handle(sequence).await;

    // Assert
    assert!(result.is_ok());

    // Verify Mapping
    let mappings = fake_mapping.mappings.lock().unwrap();
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].0, command_id);
    assert_eq!(mappings[0].1, client_reference);
    assert_eq!(mappings[0].3, "journalpost");

    // Verify Dispatch
    let dispatched = fake_dispatcher.dispatched.lock().unwrap();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].command_id, command_id);
}

#[tokio::test]
async fn test_ingest_command_internt_notat_journalpost_success() {
    // Arrange
    let fake_mapping = FakeIdMappingRepository::default();
    let fake_dispatcher = FakeCommandDispatcher::default();

    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    let command = Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
        felles: JournalpostCommon {
            client_reference,
            tittel: "Internt notat".to_string(),
            dokument_dato: "2023-01-03".to_string(),
            saksbehandler: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: None,
            dokumenter: vec![],
            sak_key: SakKey::ClientReference(client_reference),
            kildesystem: None,
        },
    });

    let envelope = CommandEnvelope {
        command_id,
        correlation_id: Some(Uuid::new_v4()),
        payload: command,
    };
    let sequence = CommandSequence::try_from(vec![envelope]).unwrap();

    let service = IngestCommandService::new(
        Box::new(fake_mapping.clone()),
        Box::new(fake_dispatcher.clone()),
    );

    // Act
    let result: anyhow::Result<()> = service.handle(sequence).await;

    // Assert
    assert!(result.is_ok());

    // Verify Mapping
    let mappings = fake_mapping.mappings.lock().unwrap();
    assert_eq!(mappings.len(), 1);
    assert_eq!(mappings[0].0, command_id);
    assert_eq!(mappings[0].1, client_reference);
    assert_eq!(mappings[0].3, "journalpost");

    // Verify Dispatch
    let dispatched = fake_dispatcher.dispatched.lock().unwrap();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].command_id, command_id);
}
