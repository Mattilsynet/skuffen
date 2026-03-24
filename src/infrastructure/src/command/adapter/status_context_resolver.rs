use std::fmt::Write;

use application::command::ports::{
    id_mapping_port::IdMappingRepository, status_context_port::CommandStatusContextResolver,
};
use async_trait::async_trait;
use domain::eksekvering::typer::CommandLifecycleContext;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::query::queries::SakKey;
use uuid::Uuid;

pub struct IdMappingStatusContextResolver {
    id_mapping: Box<dyn IdMappingRepository>,
}

impl IdMappingStatusContextResolver {
    pub fn new(id_mapping: Box<dyn IdMappingRepository>) -> Self {
        Self { id_mapping }
    }

    async fn resolve_arkiv_id_from_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<String>, anyhow::Error> {
        let skuffen_id = match self
            .id_mapping
            .hent_skuffen_id_fra_mapping(client_reference)
            .await?
        {
            Some(skuffen_id) => skuffen_id,
            None => return Ok(None),
        };

        self.id_mapping.hent_arkiv_id_fra_mapping(skuffen_id).await
    }

    async fn resolve_saksnummer(&self, sak_key: &SakKey) -> Result<Option<String>, anyhow::Error> {
        match sak_key {
            SakKey::ArkivId(saksnummer) => Ok(Some(saksnummer.as_str().to_string())),
            SakKey::ClientReference(client_reference) => {
                self.resolve_arkiv_id_from_client_reference(*client_reference)
                    .await
            }
        }
    }

    async fn resolve_dokument_ids(
        &self,
        dokumenter: &[lib_schemas::skuffen::dokument::Dokument],
    ) -> Result<Vec<String>, anyhow::Error> {
        let mut dokument_ids = Vec::new();

        for dokument in dokumenter {
            if let Some(dokument_id) = self
                .resolve_arkiv_id_from_client_reference(dokument.client_reference)
                .await?
            {
                dokument_ids.push(dokument_id);
            }
        }

        Ok(dokument_ids)
    }

    pub async fn build_reference_detail(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<Option<String>, anyhow::Error> {
        let context = self.resolve_context(envelope).await?;

        if context.is_empty() {
            return Ok(None);
        }

        let mut detail = String::new();
        if let Some(saksnummer) = context.saksnummer {
            let _ = write!(detail, "saksnummer={saksnummer}");
        }
        if let Some(journalpost_id) = context.journalpost_id {
            if !detail.is_empty() {
                detail.push(' ');
            }
            let _ = write!(detail, "journalpostId={journalpost_id}");
        }
        if !context.dokument_ids.is_empty() {
            if !detail.is_empty() {
                detail.push(' ');
            }
            let _ = write!(detail, "dokumentIds={}", context.dokument_ids.join(","));
        }

        if detail.is_empty() {
            Ok(None)
        } else {
            Ok(Some(detail))
        }
    }
}

#[async_trait]
impl CommandStatusContextResolver for IdMappingStatusContextResolver {
    async fn resolve_context(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<CommandLifecycleContext, anyhow::Error> {
        let mut context = CommandLifecycleContext::default();

        match &envelope.payload {
            Command::OpprettSak(cmd) => {
                context.sak_client_reference = Some(cmd.client_reference.to_string());
                context.saksnummer = self
                    .resolve_arkiv_id_from_client_reference(cmd.client_reference)
                    .await?;
            }
            Command::OpprettInngåendeJournalpost(cmd) => {
                if let SakKey::ClientReference(client_reference) = &cmd.felles.sak_key {
                    context.sak_client_reference = Some(client_reference.to_string());
                }
                context.saksnummer = self.resolve_saksnummer(&cmd.felles.sak_key).await?;
                context.journalpost_client_reference =
                    Some(cmd.felles.client_reference.to_string());
                context.journalpost_id = self
                    .resolve_arkiv_id_from_client_reference(cmd.felles.client_reference)
                    .await?;
                context.dokument_client_references = cmd
                    .felles
                    .dokumenter
                    .iter()
                    .map(|dokument| dokument.client_reference.to_string())
                    .collect();
                context.dokument_ids = self.resolve_dokument_ids(&cmd.felles.dokumenter).await?;
            }
            Command::OpprettUtgåendeJournalpost(cmd) => {
                if let SakKey::ClientReference(client_reference) = &cmd.felles.sak_key {
                    context.sak_client_reference = Some(client_reference.to_string());
                }
                context.saksnummer = self.resolve_saksnummer(&cmd.felles.sak_key).await?;
                context.journalpost_client_reference =
                    Some(cmd.felles.client_reference.to_string());
                context.journalpost_id = self
                    .resolve_arkiv_id_from_client_reference(cmd.felles.client_reference)
                    .await?;
                context.dokument_client_references = cmd
                    .felles
                    .dokumenter
                    .iter()
                    .map(|dokument| dokument.client_reference.to_string())
                    .collect();
                context.dokument_ids = self.resolve_dokument_ids(&cmd.felles.dokumenter).await?;
            }
            Command::OpprettInterntNotatJournalpost(cmd) => {
                if let SakKey::ClientReference(client_reference) = &cmd.felles.sak_key {
                    context.sak_client_reference = Some(client_reference.to_string());
                }
                context.saksnummer = self.resolve_saksnummer(&cmd.felles.sak_key).await?;
                context.journalpost_client_reference =
                    Some(cmd.felles.client_reference.to_string());
                context.journalpost_id = self
                    .resolve_arkiv_id_from_client_reference(cmd.felles.client_reference)
                    .await?;
                context.dokument_client_references = cmd
                    .felles
                    .dokumenter
                    .iter()
                    .map(|dokument| dokument.client_reference.to_string())
                    .collect();
                context.dokument_ids = self.resolve_dokument_ids(&cmd.felles.dokumenter).await?;
            }
            Command::AvsluttSak(cmd) => {
                if let SakKey::ClientReference(client_reference) = &cmd.sak_key {
                    context.sak_client_reference = Some(client_reference.to_string());
                }
                context.saksnummer = self.resolve_saksnummer(&cmd.sak_key).await?;
            }
        }

        Ok(context)
    }
}
