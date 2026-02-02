use crate::ports::command_dispatcher_port::CommandDispatcher;
use crate::ports::id_mapping_port::IdMappingRepository;
use crate::services::ingest_command::IngestCommandService;
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{CommandEnvelope, CommandSequence, Kommando};
use lib_schemas::skuffen::command::sak::OpprettSak;
use lib_schemas::skuffen::sak::{Ordningsverdi, Sakstittel};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// --- Fakes ---

#[derive(Clone, Default)]
struct FakeIdMappingRepository {
    pub mappings: Arc<Mutex<Vec<(Uuid, Uuid, String, Option<String>)>>>,
    pub should_fail: bool,
}

#[async_trait]
impl IdMappingRepository for FakeIdMappingRepository {
    async fn register_mapping(
        &self,
        command_id: Uuid,
        skuffen_id: Uuid,
        entity_type: String,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        if self.should_fail {
            return Err(anyhow::anyhow!("DB Error"));
        }
        let mut store = self.mappings.lock().unwrap();
        store.push((command_id, skuffen_id, entity_type, arkiv_id));
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeCommandDispatcher {
    pub dispatched: Arc<Mutex<Vec<CommandEnvelope<Kommando>>>>,
    pub should_fail: bool,
}

#[async_trait]
impl CommandDispatcher for FakeCommandDispatcher {
    async fn dispatch(&self, command: &CommandEnvelope<Kommando>) -> Result<(), anyhow::Error> {
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
async fn test_ingest_command_success() {
    // Arrange
    let fake_mapping = FakeIdMappingRepository::default();
    let fake_dispatcher = FakeCommandDispatcher::default();

    let command_id = Uuid::new_v4();
    let command = Kommando::OpprettSak(OpprettSak {
        sakstittel: Sakstittel("Test Sak".to_string()),
        ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
        arkivdel: None,
        journalenhet: None,
        saksbehandler: Some("Z99999".to_string()),
        saksbehandler_enhet: None,
        tilgang: None,
        virksomhetsmappe_id: None,
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
    assert_eq!(mappings[0].0, command_id); // command_id matches
    assert_eq!(mappings[0].2, "sak"); // entity_type correct

    // Verify Dispatch
    let dispatched = fake_dispatcher.dispatched.lock().unwrap();
    assert_eq!(dispatched.len(), 1);
    assert_eq!(dispatched[0].command_id, command_id);
}

#[tokio::test]
async fn test_ingest_command_mapping_failure() {
    // Arrange
    let mut fake_mapping = FakeIdMappingRepository::default();
    fake_mapping.should_fail = true;

    let fake_dispatcher = FakeCommandDispatcher::default();

    let command_id = Uuid::new_v4();
    let command = Kommando::OpprettSak(OpprettSak {
        sakstittel: Sakstittel("Test Sak".to_string()),
        ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
        arkivdel: None,
        journalenhet: None,
        saksbehandler: None,
        saksbehandler_enhet: None,
        tilgang: None,
        virksomhetsmappe_id: None,
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
    let command = Kommando::OpprettSak(OpprettSak {
        sakstittel: Sakstittel("Test Sak".to_string()),
        ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
        arkivdel: None,
        journalenhet: None,
        saksbehandler: None,
        saksbehandler_enhet: None,
        tilgang: None,
        virksomhetsmappe_id: None,
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
