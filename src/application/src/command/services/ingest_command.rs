use anyhow::{Context, Result};
use domain::eksekvering::typer::CommandLifecycleContext;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope, CommandSequence};
use lib_schemas::skuffen::dokument::Dokument;
use uuid::Uuid;

use crate::command::ports::{
    command_dispatcher_port::CommandDispatcher,
    id_mapping_port::{IdMappingRepository, MappingEntityType},
    status_publisher_port::CommandStatusPublisher,
};
use crate::command::status::mottatt_event;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenSakId};

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
    /// Returns Ok if all processed (or idempotently skipped).
    pub async fn handle(&self, commands: CommandSequence) -> Result<()> {
        for envelope in commands {
            self.process_command(envelope).await?;
        }
        Ok(())
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
                    &envelope.payload,
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

    fn build_initial_context(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> CommandLifecycleContext {
        let mut context = CommandLifecycleContext::default();

        match &envelope.payload {
            Command::OpprettSak(command) => {
                context.sak_client_reference = Some(command.client_reference.to_string());
            }
            Command::OpprettInngåendeJournalpost(command) => {
                self.populate_journalpost_context(&mut context, &command.felles)
            }
            Command::OpprettUtgåendeJournalpost(command) => {
                self.populate_journalpost_context(&mut context, &command.felles)
            }
            Command::OpprettInterntNotatJournalpost(command) => {
                self.populate_journalpost_context(&mut context, &command.felles)
            }
            Command::AvsluttSak(command) => match &command.sak_key {
                lib_schemas::skuffen::query::queries::SakKey::ArkivId(saksnummer) => {
                    context.saksnummer = Some(saksnummer.as_str().to_string());
                }
                lib_schemas::skuffen::query::queries::SakKey::ClientReference(client_reference) => {
                    context.sak_client_reference = Some(client_reference.to_string());
                }
            },
            Command::SettSaksansvarlig(command) => match &command.sak_key {
                lib_schemas::skuffen::query::queries::SakKey::ArkivId(saksnummer) => {
                    context.saksnummer = Some(saksnummer.as_str().to_string());
                }
                lib_schemas::skuffen::query::queries::SakKey::ClientReference(client_reference) => {
                    context.sak_client_reference = Some(client_reference.to_string());
                }
            },
        }

        context
    }

    fn populate_journalpost_context(
        &self,
        context: &mut CommandLifecycleContext,
        felles: &lib_schemas::skuffen::command::journalpost::JournalpostCommon,
    ) {
        context.journalpost_client_reference = Some(felles.client_reference.to_string());

        match &felles.sak_key {
            lib_schemas::skuffen::query::queries::SakKey::ArkivId(saksnummer) => {
                context.saksnummer = Some(saksnummer.as_str().to_string());
            }
            lib_schemas::skuffen::query::queries::SakKey::ClientReference(client_reference) => {
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
            Command::OpprettInngåendeJournalpost(c) => Some(c.felles.client_reference),
            Command::OpprettUtgåendeJournalpost(c) => Some(c.felles.client_reference),
            Command::OpprettInterntNotatJournalpost(c) => Some(c.felles.client_reference),
            Command::AvsluttSak(_) => None, // No new client reference
            Command::SettSaksansvarlig(_) => None, // No new client reference
        }
    }

    fn extract_documents<'a>(&self, command: &'a Command) -> Option<&'a Vec<Dokument>> {
        match command {
            Command::OpprettInngåendeJournalpost(c) => Some(&c.felles.dokumenter),
            Command::OpprettUtgåendeJournalpost(c) => Some(&c.felles.dokumenter),
            Command::OpprettInterntNotatJournalpost(c) => Some(&c.felles.dokumenter),
            _ => None,
        }
    }

    fn extract_arkiv_id(&self, command: &Command) -> Option<String> {
        let sak_key = match command {
            Command::OpprettInngåendeJournalpost(c) => Some(&c.felles.sak_key),
            Command::OpprettUtgåendeJournalpost(c) => Some(&c.felles.sak_key),
            Command::OpprettInterntNotatJournalpost(c) => Some(&c.felles.sak_key),
            Command::AvsluttSak(c) => Some(&c.sak_key),
            Command::SettSaksansvarlig(c) => Some(&c.sak_key),
            Command::OpprettSak(_) => None,
        }?;

        match sak_key {
            lib_schemas::skuffen::query::queries::SakKey::ArkivId(saksnummer) => {
                Some(saksnummer.as_str().to_string())
            }
            _ => None,
        }
    }
}
