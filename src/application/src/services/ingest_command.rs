use anyhow::{Context, Result};
use lib_schemas::skuffen::command::commands::{CommandEnvelope, CommandSequence, Kommando};
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

        // 1. Idempotency / ID Mapping
        // We register (command_id) -> (skuffen_id)
        self.id_mapping
            .register_mapping(
                command_id,
                skuffen_id,
                entity_type.to_string(),
                None, // arkiv_id unknown yet
            )
            .await
            .context("Failed to register id_mapping")?;

        // 2. Dispatch
        self.dispatcher
            .dispatch(&envelope)
            .await
            .context("Failed to dispatch command")?;

        Ok(())
    }
}
