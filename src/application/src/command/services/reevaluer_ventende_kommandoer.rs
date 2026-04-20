use anyhow::{Context, Result};
use domain::eksekvering::tilstand::{er_ferdig, neste_handling};
use domain::eksekvering::typer::{command_metadata, done_subject, EksekveringFeiltype};
use lib_schemas::skuffen::status::SkuffenStatusErrorCode;

use crate::command::ports::command_execution_port::{
    CommandExecutionRepository, EksekveringKommando,
};
use crate::command::ports::eksekvering_port::{
    EksekveringKvitteringPublisher, EksekveringStatusPublisher,
};
use crate::command::ports::entity_tilstand_port::EntityTilstandRepository;
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::ports::ventende_kommando_wakeup_port::VentendeKommandoWakeup;
use crate::command::services::execution_registration::resolve_registration;
use crate::command::status::utfores_error_event;

pub struct ReevaluerVentendeKommandoerService {
    execution_repo: Box<dyn CommandExecutionRepository>,
    entity_tilstand_repo: Box<dyn EntityTilstandRepository>,
    id_mapping_repo: Box<dyn IdMappingRepository>,
    status_publisher: Box<dyn EksekveringStatusPublisher>,
    done_publisher: Box<dyn EksekveringKvitteringPublisher>,
    outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
}

impl ReevaluerVentendeKommandoerService {
    pub fn new(
        execution_repo: Box<dyn CommandExecutionRepository>,
        entity_tilstand_repo: Box<dyn EntityTilstandRepository>,
        id_mapping_repo: Box<dyn IdMappingRepository>,
        status_publisher: Box<dyn EksekveringStatusPublisher>,
        done_publisher: Box<dyn EksekveringKvitteringPublisher>,
        outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
    ) -> Self {
        Self {
            execution_repo,
            entity_tilstand_repo,
            id_mapping_repo,
            status_publisher,
            done_publisher,
            outward_status_projector,
        }
    }

    pub async fn etter_sak_endret(
        &self,
        sak_id: domain::eksekvering::id::SkuffenSakId,
    ) -> Result<()> {
        let ventende = self
            .execution_repo
            .hent_blokkert_venter_for_sak(sak_id)
            .await?;
        self.reevaluer(ventende).await
    }

    pub async fn etter_journalpost_endret(
        &self,
        _journalpost_id: domain::eksekvering::id::SkuffenJournalpostId,
    ) -> Result<()> {
        // TODO: Tilstandsmodellen er sak-scopet, så journalpost-endringer
        // bør trigge wakeup via sak_id. Implementeres når sak-lookup for
        // journalpost er tilgjengelig.
        Ok(())
    }

    async fn reevaluer(&self, ventende: Vec<EksekveringKommando>) -> Result<()> {
        for kommando in ventende {
            self.reevaluer_kommando(kommando).await?;
        }

        Ok(())
    }

    async fn reevaluer_kommando(&self, kommando: EksekveringKommando) -> Result<()> {
        let registration = resolve_registration(self.id_mapping_repo.as_ref(), &kommando.envelope)
            .await
            .with_context(|| {
                format!(
                    "Klarte ikke resolve registration for ventende command {}",
                    kommando.command_id
                )
            })?;

        let sak_id = registration.sak_id();
        let (command_type, _) = command_metadata(&kommando.envelope.payload);

        let Some(sak_id) = sak_id else {
            self.execution_repo
                .oppdater_til_feil(kommando.command_id, "Mangler sak_id for reevaluering")
                .await?;
            self.publiser_terminal_feil(&kommando, "Mangler sak_id for reevaluering")
                .await?;
            return Ok(());
        };

        let Some(sak_med_barn) = self.entity_tilstand_repo.hent_sak_med_barn(sak_id).await? else {
            // Sak finnes ikke ennå i tilstandstabellen — forblir blokkert
            return Ok(());
        };

        match neste_handling(command_type, &sak_med_barn) {
            Ok(Some(_)) => {
                self.execution_repo
                    .oppdater_til_klar(kommando.command_id)
                    .await?;
            }
            Ok(None) => {
                if er_ferdig(&sak_med_barn) {
                    // Alt er realisert — kommandoen kan kjøres for å fullføre
                    self.execution_repo
                        .oppdater_til_klar(kommando.command_id)
                        .await?;
                }
                // Ellers: forblir blokkert
            }
            Err(feil) => match feil.feiltype {
                EksekveringFeiltype::Blocked => {
                    // Forblir blokkert
                }
                _ => {
                    let detalj = feil.melding.clone();
                    self.execution_repo
                        .oppdater_til_feil(kommando.command_id, &detalj)
                        .await?;
                    self.publiser_terminal_feil(&kommando, &detalj).await?;
                }
            },
        }

        Ok(())
    }

    async fn publiser_terminal_feil(
        &self,
        kommando: &EksekveringKommando,
        detalj: &str,
    ) -> Result<()> {
        let context = self
            .outward_status_projector
            .resolve_context(&kommando.envelope)
            .await?;
        let event = utfores_error_event(
            &kommando.envelope,
            detalj,
            Some(SkuffenStatusErrorCode::ProcessingFailed),
            context,
            Some((kommando.attempt_no.max(1)) as u32),
        );
        self.status_publisher.publiser_status(event).await?;
        if kommando.utfores_venter_publisert {
            let (subject, _) = done_subject(&kommando.envelope);
            self.done_publisher
                .publiser_done(&subject, &kommando.envelope)
                .await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl VentendeKommandoWakeup for ReevaluerVentendeKommandoerService {
    async fn etter_sak_endret(&self, sak_id: domain::eksekvering::id::SkuffenSakId) -> Result<()> {
        Self::etter_sak_endret(self, sak_id).await
    }

    async fn etter_journalpost_endret(
        &self,
        journalpost_id: domain::eksekvering::id::SkuffenJournalpostId,
    ) -> Result<()> {
        Self::etter_journalpost_endret(self, journalpost_id).await
    }
}
