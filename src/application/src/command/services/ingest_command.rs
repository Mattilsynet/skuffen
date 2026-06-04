use anyhow::{Context, Result};
use domain::eksekvering::typer::CommandLifecycleContext;
use uuid::Uuid;

use crate::command::ports::{
    command_dispatcher_port::CommandDispatcher,
    id_mapping_port::{IdMappingRepository, MappingEntityType},
    status_publisher_port::CommandStatusPublisher,
};
use crate::command::status::mottatt_event;
use crate::command::{Command, CommandEnvelope, Dokument, JournalpostCommon, SakKey};
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenSakId};

pub trait IntoCommandBatch {
    fn into_command_batch(self) -> Vec<CommandEnvelope<Command>>;
}

impl IntoCommandBatch for Vec<CommandEnvelope<Command>> {
    fn into_command_batch(self) -> Vec<CommandEnvelope<Command>> {
        self
    }
}

pub struct IngestCommandService {
    id_mapping: Box<dyn IdMappingRepository>,
    dispatcher: Box<dyn CommandDispatcher>,
    status_publisher: Box<dyn CommandStatusPublisher>,
}

impl IngestCommandService {
    pub fn new(
        id_mapping: Box<dyn IdMappingRepository>,
        dispatcher: Box<dyn CommandDispatcher>,
        status_publisher: Box<dyn CommandStatusPublisher>,
    ) -> Self {
        Self {
            id_mapping,
            dispatcher,
            status_publisher,
        }
    }

    /// Handles a batch of commands.
    /// Returns all submitted command IDs on success, preserving order.
    /// Includes IDs for commands that are idempotently accepted/skipped.
    /// Returns Err if any command fails (no partial list).
    pub async fn handle(&self, commands: impl IntoCommandBatch) -> Result<Vec<Uuid>> {
        let mut command_ids = Vec::new();

        for envelope in commands.into_command_batch() {
            let command_id = envelope.command_id;
            self.process_command(envelope).await?;
            command_ids.push(command_id);
        }

        Ok(command_ids)
    }

    async fn process_command(&self, envelope: CommandEnvelope<Command>) -> Result<()> {
        let command_id = envelope.command_id;
        let mottatt_context = self.build_initial_context(&envelope);

        // 1. Check Command Idempotency
        if self.id_mapping.has_processed_command(command_id).await? {
            // Command already processed, idempotent success.
            return Ok(());
        }

        let skuffen_id = Uuid::now_v7(); // Generate internal ID

        // Extract Client Reference
        let client_reference = self.extract_client_reference(&envelope.payload);

        // 2. Idempotency / ID Mapping
        // We register (command_id) -> (skuffen_id) with client_reference
        if let Some(client_ref) = client_reference {
            self.id_mapping
                .register_mapping(
                    command_id,
                    client_ref,
                    SkuffenSakId::from(skuffen_id),
                    self.mapping_entity_type(&envelope.payload),
                    None, // arkiv_id unknown yet
                )
                .await
                .context("Failed to register id_mapping")?;

            // Register documents mappings
            if let Some(documents) = self.extract_documents(&envelope.payload) {
                for doc in documents {
                    let doc_skuffen_id = Uuid::now_v7();
                    self.id_mapping
                        .register_document_mapping(
                            command_id,
                            doc.client_reference,
                            SkuffenDokumentId::from(doc_skuffen_id),
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

        if let Some(arkiv_id) = self.extract_arkiv_id(&envelope.payload) {
            let _ = self
                .id_mapping
                .hent_eller_opprett_skuffen_id_for_arkiv_id(
                    MappingEntityType::Sak,
                    arkiv_id.as_str(),
                )
                .await;
        }

        // 3. Dispatch
        self.dispatcher
            .dispatch(&envelope)
            .await
            .context("Failed to dispatch command")?;

        self.status_publisher
            .publish_status(mottatt_event(&envelope, mottatt_context))
            .await
            .context("Failed to publish mottatt status")?;

        Ok(())
    }

    fn mapping_entity_type(&self, command: &Command) -> MappingEntityType {
        match command {
            Command::OpprettSak(_) | Command::AvsluttSak(_) | Command::SettSaksansvarlig(_) => {
                MappingEntityType::Sak
            }
            Command::OpprettInngaaendeJournalpost(_)
            | Command::OpprettUtgaaendeJournalpost(_)
            | Command::OpprettInterntNotatJournalpost(_) => MappingEntityType::Journalpost,
        }
    }

    fn build_initial_context(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> CommandLifecycleContext {
        let mut context = CommandLifecycleContext::default();

        match &envelope.payload {
            Command::OpprettSak(command) => {
                context.sak_client_reference = Some(command.client_reference.to_string());
            }
            Command::OpprettInngaaendeJournalpost(command) => {
                self.populate_journalpost_context(&mut context, &command.felles)
            }
            Command::OpprettUtgaaendeJournalpost(command) => {
                self.populate_journalpost_context(&mut context, &command.felles)
            }
            Command::OpprettInterntNotatJournalpost(command) => {
                self.populate_journalpost_context(&mut context, &command.felles)
            }
            Command::AvsluttSak(command) => match &command.sak_key {
                SakKey::ArkivId(saksnummer) => {
                    context.saksnummer = Some(saksnummer.clone());
                }
                SakKey::ClientReference(client_reference) => {
                    context.sak_client_reference = Some(client_reference.to_string());
                }
            },
            Command::SettSaksansvarlig(command) => match &command.sak_key {
                SakKey::ArkivId(saksnummer) => {
                    context.saksnummer = Some(saksnummer.clone());
                }
                SakKey::ClientReference(client_reference) => {
                    context.sak_client_reference = Some(client_reference.to_string());
                }
            },
        }

        context
    }

    fn populate_journalpost_context(
        &self,
        context: &mut CommandLifecycleContext,
        felles: &JournalpostCommon,
    ) {
        context.journalpost_client_reference = Some(felles.client_reference.to_string());

        match &felles.sak_key {
            SakKey::ArkivId(saksnummer) => {
                context.saksnummer = Some(saksnummer.clone());
            }
            SakKey::ClientReference(client_reference) => {
                context.sak_client_reference = Some(client_reference.to_string());
            }
        }

        context.dokument_client_references = felles
            .dokumenter
            .iter()
            .map(|dokument| dokument.client_reference.to_string())
            .collect();
        context.dokument_ids = felles
            .dokumenter
            .iter()
            .map(|dokument| dokument.client_reference.to_string())
            .collect();
    }

    fn extract_client_reference(&self, command: &Command) -> Option<Uuid> {
        match command {
            Command::OpprettSak(c) => Some(c.client_reference),
            Command::OpprettInngaaendeJournalpost(c) => Some(c.felles.client_reference),
            Command::OpprettUtgaaendeJournalpost(c) => Some(c.felles.client_reference),
            Command::OpprettInterntNotatJournalpost(c) => Some(c.felles.client_reference),
            Command::AvsluttSak(_) => None, // No new client reference
            Command::SettSaksansvarlig(_) => None, // No new client reference
        }
    }

    fn extract_documents<'a>(&self, command: &'a Command) -> Option<&'a Vec<Dokument>> {
        match command {
            Command::OpprettInngaaendeJournalpost(c) => Some(&c.felles.dokumenter),
            Command::OpprettUtgaaendeJournalpost(c) => Some(&c.felles.dokumenter),
            Command::OpprettInterntNotatJournalpost(c) => Some(&c.felles.dokumenter),
            Command::OpprettSak(_) | Command::AvsluttSak(_) | Command::SettSaksansvarlig(_) => None,
        }
    }

    fn extract_arkiv_id(&self, command: &Command) -> Option<String> {
        let sak_key = match command {
            Command::OpprettInngaaendeJournalpost(c) => Some(&c.felles.sak_key),
            Command::OpprettUtgaaendeJournalpost(c) => Some(&c.felles.sak_key),
            Command::OpprettInterntNotatJournalpost(c) => Some(&c.felles.sak_key),
            Command::AvsluttSak(c) => Some(&c.sak_key),
            Command::SettSaksansvarlig(c) => Some(&c.sak_key),
            Command::OpprettSak(_) => None,
        }?;

        match sak_key {
            SakKey::ArkivId(saksnummer) => Some(saksnummer.clone()),
            SakKey::ClientReference(_) => None,
        }
    }
}
