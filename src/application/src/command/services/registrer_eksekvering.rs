use anyhow::Result;
use async_trait::async_trait;
use domain::eksekvering::typer::CommandLifecycleEvent;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use uuid::Uuid;

use crate::command::ports::eksekvering_port::EksekveringStatusPublisher;
use crate::command::ports::eksekvering_state_port::{
    EksekveringStateRepository, JournalpostState, SakState, SakStatus,
};
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::registrer_eksekvering_port::RegistrerEksekveringUseCase;
use crate::command::ports::status_context_port::CommandStatusContextResolver;
use crate::command::status::utfores_venter_event;
use domain::eksekvering::plan::{EksekveringsPlan, Steg, Utsending};

pub struct RegistrerEksekveringService {
    state_repo: Box<dyn EksekveringStateRepository>,
    id_mapping_repo: Box<dyn IdMappingRepository>,
    status_publisher: Box<dyn EksekveringStatusPublisher>,
    status_context_resolver: Box<dyn CommandStatusContextResolver>,
}

impl RegistrerEksekveringService {
    pub fn new(
        state_repo: Box<dyn EksekveringStateRepository>,
        id_mapping_repo: Box<dyn IdMappingRepository>,
        status_publisher: Box<dyn EksekveringStatusPublisher>,
        status_context_resolver: Box<dyn CommandStatusContextResolver>,
    ) -> Self {
        Self {
            state_repo,
            id_mapping_repo,
            status_publisher,
            status_context_resolver,
        }
    }

    async fn ensure_sak_state(&self, envelope: &CommandEnvelope<Command>) -> Result<()> {
        let plan = EksekveringsPlan::fra_command(&envelope.payload)
            .map_err(|err| anyhow::anyhow!(err.melding))?;

        if let Some(Steg::OpprettJournalpost { plan }) = plan.steg.first() {
            let sak_id = self.resolve_sak_id(&plan.sak_key).await?;
            let journalpost_id = self
                .resolve_skuffen_id_for_client_reference("journalpost", plan.journalpost_id)
                .await?;

            let sak_state = match &plan.sak_key {
                DtoSakKey::ClientReference(_) => SakState {
                    status: SakStatus::UnderBehandling,
                    opprettet: false,
                    saksnummer: None,
                },
                DtoSakKey::ArkivId(saksnummer) => SakState {
                    status: SakStatus::UnderBehandling,
                    opprettet: true,
                    saksnummer: Some(saksnummer.as_str().to_string()),
                },
            };

            if self
                .state_repo
                .hent_sak_state_fra_state(sak_id)
                .await?
                .is_none()
            {
                self.state_repo.lagre_sak_state(sak_id, sak_state).await?;
            }

            if self
                .state_repo
                .hent_journalpost_state_fra_state(journalpost_id)
                .await?
                .is_none()
            {
                let journalpost_state = JournalpostState {
                    journalfoert: false,
                    avskrevet: false,
                    ekspedert: false,
                    har_feilede_dokumenter: false,
                    med_utsending: matches!(plan.utsending, Some(Utsending::MedUtsending)),
                    journalposttype: plan.journalpost_type,
                    journalpostnummer: None,
                };
                self.state_repo
                    .lagre_journalpost_state(journalpost_id, sak_id, journalpost_state)
                    .await?;
            }
        }

        Ok(())
    }

    async fn resolve_sak_id(&self, sak_key: &DtoSakKey) -> Result<Uuid> {
        match sak_key {
            DtoSakKey::ClientReference(client_reference) => {
                self.resolve_skuffen_id_for_client_reference("sak", *client_reference)
                    .await
            }
            DtoSakKey::ArkivId(saksnummer) => {
                self.id_mapping_repo
                    .hent_eller_opprett_skuffen_id_for_arkiv_id("sak", saksnummer.as_str())
                    .await
            }
        }
    }

    async fn resolve_skuffen_id_for_client_reference(
        &self,
        entity_type: &str,
        client_reference: Uuid,
    ) -> Result<Uuid> {
        match self
            .id_mapping_repo
            .hent_skuffen_id_fra_mapping(client_reference)
            .await?
        {
            Some(skuffen_id) => Ok(skuffen_id),
            None => Err(anyhow::anyhow!(
                "Fant ikke skuffen_id for {entity_type} client_reference {client_reference}"
            )),
        }
    }

    async fn emit_status(&self, event: CommandLifecycleEvent) -> Result<()> {
        self.status_publisher.publiser_status(event).await
    }
}

#[async_trait]
impl RegistrerEksekveringUseCase for RegistrerEksekveringService {
    async fn handle(&self, envelope: &CommandEnvelope<Command>) -> Result<()> {
        self.ensure_sak_state(envelope).await?;
        let inserted = self.state_repo.registrer_kommando(envelope).await?;

        if inserted {
            let context = self
                .status_context_resolver
                .resolve_context(envelope)
                .await?;
            self.emit_status(utfores_venter_event(envelope, context, Some(1)))
                .await?;
        }

        Ok(())
    }
}
