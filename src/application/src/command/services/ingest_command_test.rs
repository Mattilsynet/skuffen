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
use lib_schemas::skuffen::dokument::Dokument;

use lib_schemas::skuffen::query::queries::SakKey;
use lib_schemas::skuffen::sak::{Ordningsverdi, Sakstittel};
use std::collections::HashSet;
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
        command: &Command,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        if self.should_fail {
            return Err(anyhow::anyhow!("DB Error"));
        }
        let entity_type = match command {
            Command::OpprettSak(_) | Command::AvsluttSak(_) => "sak",
            Command::OpprettInngåendeJournalpost(_)
            | Command::OpprettUtgåendeJournalpost(_)
            | Command::OpprettInterntNotatJournalpost(_) => "journalpost",
        };
        let mut store = self.mappings.lock().unwrap();
        if let Some((_, _, existing_skuffen_id, _, _)) = store
            .iter()
            .find(|(_, existing_client_ref, _, _, _)| *existing_client_ref == client_reference)
        {
            if *existing_skuffen_id != skuffen_id {
                return Err(anyhow::anyhow!(
                    "client_reference is already mapped to a different skuffen_id"
                ));
            }
            return Ok(());
        }
        store.push((
            command_id,
            client_reference,
            skuffen_id,
            entity_type.to_string(),
            arkiv_id,
        ));
        Ok(())
    }

    async fn register_document_mapping(
        &self,
        command_id: Uuid,
        client_reference: Uuid,
        skuffen_id: Uuid,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        if self.should_fail {
            return Err(anyhow::anyhow!("DB Error"));
        }
        let mut store = self.mappings.lock().unwrap();
        if let Some((_, _, existing_skuffen_id, _, _)) = store
            .iter()
            .find(|(_, existing_client_ref, _, _, _)| *existing_client_ref == client_reference)
        {
            if *existing_skuffen_id != skuffen_id {
                return Err(anyhow::anyhow!(
                    "client_reference is already mapped to a different skuffen_id"
                ));
            }
            return Ok(());
        }
        store.push((
            command_id,
            client_reference,
            skuffen_id,
            "dokument".to_string(),
            arkiv_id,
        ));
        Ok(())
    }

    async fn oppdater_arkiv_id_for_client_reference(
        &self,
        _client_reference: Uuid,
        _arkiv_id: String,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn hent_arkiv_id_fra_mapping(&self, _skuffen_id: Uuid) -> Result<Option<String>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_skuffen_id_fra_mapping(&self, _client_reference: Uuid) -> Result<Option<Uuid>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_skuffen_id_fra_arkiv_id_i_mapping(
        &self,
        _arkiv_id: &str,
    ) -> Result<Option<Uuid>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_eller_opprett_skuffen_id_for_arkiv_id(
        &self,
        _entity_type: &str,
        _arkiv_id: &str,
    ) -> Result<Uuid, anyhow::Error> {
        Ok(Uuid::new_v4())
    }

    async fn delete_arkiv_mapping(
        &self,
        _entity_type: &str,
        _arkiv_id: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
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
async fn test_ingest_command_registers_document_mappings() {
    // Arrange
    let fake_mapping = FakeIdMappingRepository::default();
    let fake_dispatcher = FakeCommandDispatcher::default();

    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    let documents = vec![
        Dokument {
            client_reference: Uuid::new_v4(),
            tittel: "Vedlegg 1".to_string(),
            filtype: "PDF".to_string(),
            dokument_referanse: Uuid::new_v4(),
        },
        Dokument {
            client_reference: Uuid::new_v4(),
            tittel: "Vedlegg 2".to_string(),
            filtype: "PDF".to_string(),
            dokument_referanse: Uuid::new_v4(),
        },
    ];

    let command = Command::OpprettInngåendeJournalpost(OpprettInngåendeJournalpost {
        felles: JournalpostCommon {
            client_reference,
            tittel: "Inngående brev".to_string(),
            dokument_dato: "2023-01-01".to_string(),
            saksbehandler: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: None,
            dokumenter: documents.clone(),
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

    let mappings = fake_mapping.mappings.lock().unwrap();
    assert_eq!(mappings.len(), 1 + documents.len());
    assert!(mappings.iter().all(|(cid, _, _, _, _)| *cid == command_id));

    let journalpost_count = mappings
        .iter()
        .filter(|(_, _, _, entity_type, _)| entity_type == "journalpost")
        .count();
    let dokument_count = mappings
        .iter()
        .filter(|(_, _, _, entity_type, _)| entity_type == "dokument")
        .count();
    assert_eq!(journalpost_count, 1);
    assert_eq!(dokument_count, documents.len());

    let skuffen_ids: HashSet<Uuid> = mappings.iter().map(|(_, _, sid, _, _)| *sid).collect();
    assert_eq!(skuffen_ids.len(), mappings.len());

    let client_refs: HashSet<Uuid> = mappings.iter().map(|(_, cr, _, _, _)| *cr).collect();
    assert!(client_refs.contains(&client_reference));
    for doc in documents {
        assert!(client_refs.contains(&doc.client_reference));
    }
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
async fn test_ingest_command_allows_multiple_mappings_per_command_id() {
    // Arrange
    let fake_mapping = FakeIdMappingRepository::default();
    let fake_dispatcher = FakeCommandDispatcher::default();

    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    let documents = vec![
        Dokument {
            client_reference: Uuid::new_v4(),
            tittel: "Vedlegg 1".to_string(),
            filtype: "PDF".to_string(),
            dokument_referanse: Uuid::new_v4(),
        },
        Dokument {
            client_reference: Uuid::new_v4(),
            tittel: "Vedlegg 2".to_string(),
            filtype: "PDF".to_string(),
            dokument_referanse: Uuid::new_v4(),
        },
    ];

    let command = Command::OpprettInngåendeJournalpost(OpprettInngåendeJournalpost {
        felles: JournalpostCommon {
            client_reference,
            tittel: "Inngående brev".to_string(),
            dokument_dato: "2023-01-01".to_string(),
            saksbehandler: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: None,
            dokumenter: documents.clone(),
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

    let mappings = fake_mapping.mappings.lock().unwrap();
    let command_mappings = mappings
        .iter()
        .filter(|(cid, _, _, _, _)| *cid == command_id)
        .count();
    assert_eq!(command_mappings, 1 + documents.len());
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
async fn test_ingest_command_idempotent_duplicate_command_id() {
    // Arrange
    let fake_mapping = FakeIdMappingRepository::default();
    let fake_dispatcher = FakeCommandDispatcher::default();

    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    let command = Command::OpprettSak(OpprettSak {
        client_reference,
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

    // Act - first should succeed
    let result1: anyhow::Result<()> = service.handle(sequence).await;
    assert!(result1.is_ok());

    let command2 = Command::OpprettSak(OpprettSak {
        client_reference,
        sakstittel: Sakstittel("Test Sak 2".to_string()),
        ordningsverdi: Ordningsverdi::new("456".to_string()).unwrap(),
        arkivdel: Arkivdel::Tilsynsdivisjonene,
        saksbehandler_id: "Z99999".to_string(),
        saksbehandler_enhet: "42".to_string(),
        tilgang: None,
    });

    let envelope2 = CommandEnvelope {
        command_id: command_id,
        correlation_id: Some(Uuid::new_v4()),
        payload: command2,
    };
    let sequence2 = CommandSequence::try_from(vec![envelope2]).unwrap();

    // Act - should now be idempotent based on command_id
    let result2: anyhow::Result<()> = service.handle(sequence2).await;

    // Assert
    assert!(result2.is_ok());

    let mappings = fake_mapping.mappings.lock().unwrap();
    let client_mappings = mappings
        .iter()
        .filter(|(_, client_ref, _, _, _)| *client_ref == client_reference)
        .count();
    assert_eq!(client_mappings, 1);
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
