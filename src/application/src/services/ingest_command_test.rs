use crate::ports::command_dispatcher_port::MockCommandDispatcher;
use crate::ports::id_mapping_port::MockIdMappingRepository;
use crate::services::ingest_command::IngestCommandService;
use lib_schemas::skuffen::command::commands::{CommandEnvelope, CommandSequence, Kommando};
use lib_schemas::skuffen::command::sak::OpprettSak;
use lib_schemas::skuffen::sak::{Ordningsverdi, Sakstittel};
use mockall::predicate::*;
use std::convert::TryFrom;
use uuid::Uuid;

#[tokio::test]
async fn test_ingest_command_success() {
    // Arrange
    let mut mock_mapping = MockIdMappingRepository::new();
    let mut mock_dispatcher = MockCommandDispatcher::new();

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

    // Expect ID Mapping registration
    mock_mapping
        .expect_register_mapping()
        .with(
            eq(command_id),
            always(), // skuffen_id is random, so we just accept any
            eq("sak".to_string()),
            eq(None::<String>),
        )
        .times(1)
        .returning(|_, _, _, _| Ok(()));

    // Expect Dispatch
    mock_dispatcher
        .expect_dispatch()
        .with(always()) // Matching exact envelope is hard due to clone/ownership, 'always' is sufficient if mapping checks out
        .times(1)
        .returning(|_| Ok(()));

    let service = IngestCommandService::new(Box::new(mock_mapping), Box::new(mock_dispatcher));

    // Act
    let result: anyhow::Result<()> = service.handle(sequence).await;

    // Assert
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_ingest_command_mapping_failure() {
    // Arrange
    let mut mock_mapping = MockIdMappingRepository::new();
    let mut mock_dispatcher = MockCommandDispatcher::new();

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

    // Expect ID Mapping to fail
    mock_mapping
        .expect_register_mapping()
        .times(1)
        .returning(|_, _, _, _| Err(anyhow::anyhow!("DB Error")));

    // Expect NO dispatch
    mock_dispatcher.expect_dispatch().times(0);

    let service = IngestCommandService::new(Box::new(mock_mapping), Box::new(mock_dispatcher));

    // Act
    let result: anyhow::Result<()> = service.handle(sequence).await;

    // Assert
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "Failed to register id_mapping"
    );
}

#[tokio::test]
async fn test_ingest_command_dispatch_failure() {
    // Arrange
    let mut mock_mapping = MockIdMappingRepository::new();
    let mut mock_dispatcher = MockCommandDispatcher::new();

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

    mock_mapping
        .expect_register_mapping()
        .times(1)
        .returning(|_, _, _, _| Ok(()));

    mock_dispatcher
        .expect_dispatch()
        .times(1)
        .returning(|_| Err(anyhow::anyhow!("NATS Error")));

    let service = IngestCommandService::new(Box::new(mock_mapping), Box::new(mock_dispatcher));

    // Act
    let result: anyhow::Result<()> = service.handle(sequence).await;

    // Assert
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "Failed to dispatch command"
    );
}
