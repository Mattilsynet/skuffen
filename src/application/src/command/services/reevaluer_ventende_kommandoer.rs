use anyhow::{Context, Result};
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::{planlegg_neste_handling, CommandStateDecision};
use domain::eksekvering::typer::StatusErrorCode;

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
use crate::command::services::command_state_decision::{blocked_detail, invalid_detail};
use crate::command::services::execution_registration::{
    domain_command_for_type, resolve_registration,
};
use crate::command::status::utfores_error_event;

/// Reevaluerer eksplisitte wake-up triggers for blokkerte kommandoer.
///
/// Periodisk full-rescan av `blokkert_venter` er bevisst utsatt: sak-, journalpost-
/// og dokument-faktaendringer normaliseres alle til sak-reevaluering, og repositoryet
/// oppdaterer detalj når en kommando fortsatt er blokkert. En full-rescan bør først
/// legges til hvis drift viser tapte triggers, slik at vi unngår en parallell scheduler.
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

    pub async fn etter_sak_endret(&self, sak_id: SkuffenSakId) -> Result<()> {
        let ventende = self
            .execution_repo
            .hent_blokkert_venter_for_sak(sak_id)
            .await?;
        self.reevaluer(ventende).await
    }

    pub async fn etter_journalpost_endret(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<()> {
        let Some(sak_id) = self
            .entity_tilstand_repo
            .hent_sak_id_fra_journalpost_id(journalpost_id)
            .await?
        else {
            return Ok(());
        };

        self.etter_sak_endret(sak_id).await
    }

    pub async fn etter_dokument_endret(&self, dokument_id: SkuffenDokumentId) -> Result<()> {
        let Some(journalpost_id) = self
            .entity_tilstand_repo
            .hent_journalpost_id_fra_dokument_id(dokument_id)
            .await?
        else {
            return Ok(());
        };

        self.etter_journalpost_endret(journalpost_id).await
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
        let command_type = crate::command::status::command_metadata(&kommando.envelope.payload);

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

        let domain_command =
            match domain_command_for_type(command_type, sak_id, registration.journalpost_id()) {
                Ok(command) => command,
                Err(feil) => {
                    let detalj = feil.to_string();
                    self.execution_repo
                        .oppdater_til_feil(kommando.command_id, &detalj)
                        .await?;
                    self.publiser_terminal_feil(&kommando, &detalj).await?;
                    return Ok(());
                }
            };

        match planlegg_neste_handling(&domain_command, &sak_med_barn) {
            CommandStateDecision::Ready(_) | CommandStateDecision::Done => {
                // INVARIANT: Done flyttes også til Klar. Executor eier terminal
                // success/done-publisering, så wake-up skal ikke stille-finalisere
                // en tidligere blokkert kommando.
                self.execution_repo
                    .oppdater_til_klar(kommando.command_id)
                    .await?;
            }
            CommandStateDecision::Blocked(reason) => {
                let detalj = blocked_detail(reason);
                self.execution_repo
                    .oppdater_blokkert_detail(kommando.command_id, &detalj)
                    .await?;
            }
            CommandStateDecision::Invalid(violation) => {
                // Registration queues Invalid for executor-owned terminalization, but wake-up
                // already operates on a blocked command that became impossible from fresh facts.
                let detalj = invalid_detail(violation);
                self.execution_repo
                    .oppdater_til_feil(kommando.command_id, &detalj)
                    .await?;
                self.publiser_terminal_feil(&kommando, &detalj).await?;
            }
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
            Some(StatusErrorCode::ProcessingFailed),
            context,
            Some((kommando.attempt_no.max(1)) as u32),
        );
        self.status_publisher.publiser_status(event).await?;
        if kommando.utfores_venter_publisert {
            self.done_publisher
                .publiser_done(&kommando.envelope)
                .await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl VentendeKommandoWakeup for ReevaluerVentendeKommandoerService {
    async fn etter_sak_endret(&self, sak_id: SkuffenSakId) -> Result<()> {
        Self::etter_sak_endret(self, sak_id).await
    }

    async fn etter_journalpost_endret(&self, journalpost_id: SkuffenJournalpostId) -> Result<()> {
        Self::etter_journalpost_endret(self, journalpost_id).await
    }

    async fn etter_dokument_endret(&self, dokument_id: SkuffenDokumentId) -> Result<()> {
        Self::etter_dokument_endret(self, dokument_id).await
    }
}
