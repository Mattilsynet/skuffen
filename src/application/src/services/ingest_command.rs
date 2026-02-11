use anyhow::{Context, Result};
use lib_schemas::skuffen::command::commands::{CommandEnvelope, CommandSequence, Kommando};
use lib_schemas::skuffen::dokument::Dokument;
use uuid::Uuid;

use crate::ports::{
    command_dispatcher_port::CommandDispatcher, id_mapping_port::IdMappingRepository,
};

pub struct IngestCommandService {
    id_mapping: Box<dyn IdMappingRepository>,
    dispatcher: Box<dyn CommandDispatcher>,
}

impl IngestCommandService {
    pub fn new(
        id_mapping: Box<dyn IdMappingRepository>,
        dispatcher: Box<dyn CommandDispatcher>,
    ) -> Self {
        Self {
            id_mapping,
            dispatcher,
        }
    }

    /// Handles a batch of commands.
    /// Returns Ok if all processed (or idempotently skipped).
    pub async fn handle(&self, commands: CommandSequence) -> Result<()> {
        for envelope in commands {
            self.process_command(envelope).await?;
        }
        Ok(())
    }

    async fn process_command(&self, envelope: CommandEnvelope<Kommando>) -> Result<()> {
        let command_id = envelope.command_id;

        // 1. Check Command Idempotency
        if self.id_mapping.has_processed_command(command_id).await? {
            // Command already processed, idempotent success.
            return Ok(());
        }

        //TODO: Burde denne ligge bak mer logikk?
        let skuffen_id = Uuid::now_v7(); // Generate internal ID

        // Determine entity type based on command
        //TODO: Flyttes til database-handelern
        let entity_type = match &envelope.payload {
            Kommando::OpprettSak(_) => "sak",
            Kommando::OpprettInngåendeJournalpost(_)
            | Kommando::OpprettUtgåendeJournalpost(_)
            | Kommando::OpprettInterntNotatJournalpost(_) => "journalpost",
            Kommando::AvsluttSak(_) => "sak",
        };

        // Extract Client Reference
        let client_reference = self.extract_client_reference(&envelope.payload);

        // 2. Idempotency / ID Mapping
        // We register (command_id) -> (skuffen_id) with client_reference
        if let Some(client_ref) = client_reference {
            self.id_mapping
                .register_mapping(
                    command_id,
                    client_ref,
                    skuffen_id,
                    entity_type.to_string(),
                    None, // arkiv_id unknown yet
                )
                .await
                .context("Failed to register id_mapping")?;

            // Register documents mappings
            if let Some(documents) = self.extract_documents(&envelope.payload) {
                for doc in documents {
                    let doc_skuffen_id = Uuid::now_v7();
                    self.id_mapping
                        .register_mapping(
                            command_id,
                            doc.client_reference,
                            doc_skuffen_id,
                            "dokument".to_string(),
                            None,
                        )
                        .await
                        .context("Failed to register document mapping")?;
                }
            }
        } else {
            // Commands without client_reference (e.g. AvsluttSak) cannot be registered in id_mapping
            // because they don't map to a new entity with a unique client_reference.
            // Consequently, strict command idempotency (via has_processed_command) is not persisted for these commands.
            // This is acceptable as these operations are typically idempotent by nature.
        }

        // 3. Dispatch
        self.dispatcher
            .dispatch(&envelope)
            .await
            .context("Failed to dispatch command")?;

        Ok(())
    }

    fn extract_client_reference(&self, command: &Kommando) -> Option<Uuid> {
        match command {
            Kommando::OpprettSak(c) => Some(c.client_reference),
            Kommando::OpprettInngåendeJournalpost(c) => Some(c.felles.client_reference),
            Kommando::OpprettUtgåendeJournalpost(c) => Some(c.felles.client_reference),
            Kommando::OpprettInterntNotatJournalpost(c) => Some(c.felles.client_reference),
            Kommando::AvsluttSak(_) => None, // No new client reference
        }
    }

    fn extract_documents<'a>(&self, command: &'a Kommando) -> Option<&'a Vec<Dokument>> {
        match command {
            Kommando::OpprettInngåendeJournalpost(c) => Some(&c.felles.dokumenter),
            Kommando::OpprettUtgåendeJournalpost(c) => Some(&c.felles.dokumenter),
            Kommando::OpprettInterntNotatJournalpost(c) => Some(&c.felles.dokumenter),
            _ => None,
        }
    }
}
