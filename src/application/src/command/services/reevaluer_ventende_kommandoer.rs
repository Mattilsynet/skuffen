use anyhow::{Context, Result};
use domain::eksekvering::execution::Kjorbarhet;
use domain::eksekvering::typer::done_subject;
use lib_schemas::skuffen::status::SkuffenStatusErrorCode;

use crate::command::ports::command_execution_port::{
    CommandExecutionRepository, EksekveringKommando,
};
use crate::command::ports::eksekvering_port::{
    EksekveringKvitteringPublisher, EksekveringStatusPublisher,
};
use crate::command::ports::execution_snapshot_port::EksekveringSnapshotRepository;
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::ports::ventende_kommando_wakeup_port::VentendeKommandoWakeup;
use crate::command::services::eksekveringsklarhet_vurderer::EksekveringsklarhetVurderer;
use crate::command::services::execution_registration::resolve_registration;
use crate::command::status::utfores_error_event;

pub struct ReevaluerVentendeKommandoerService {
    execution_repo: Box<dyn CommandExecutionRepository>,
    snapshot_repo: Box<dyn EksekveringSnapshotRepository>,
    id_mapping_repo: Box<dyn IdMappingRepository>,
    klarhet_vurderer: EksekveringsklarhetVurderer,
    status_publisher: Box<dyn EksekveringStatusPublisher>,
    done_publisher: Box<dyn EksekveringKvitteringPublisher>,
    outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
}

impl ReevaluerVentendeKommandoerService {
    pub fn new(
        execution_repo: Box<dyn CommandExecutionRepository>,
        snapshot_repo: Box<dyn EksekveringSnapshotRepository>,
        id_mapping_repo: Box<dyn IdMappingRepository>,
        status_publisher: Box<dyn EksekveringStatusPublisher>,
        done_publisher: Box<dyn EksekveringKvitteringPublisher>,
        outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
    ) -> Self {
        Self {
            execution_repo,
            snapshot_repo,
            id_mapping_repo,
            klarhet_vurderer: EksekveringsklarhetVurderer::new(),
            status_publisher,
            done_publisher,
            outward_status_projector,
        }
    }

    pub async fn etter_sak_endret(
        &self,
        sak_id: domain::eksekvering::id::SkuffenSakId,
    ) -> Result<()> {
        let ventende = self.execution_repo.hent_ventende_for_sak(sak_id).await?;
        self.reevaluer(ventende).await
    }

    pub async fn etter_journalpost_endret(
        &self,
        journalpost_id: domain::eksekvering::id::SkuffenJournalpostId,
    ) -> Result<()> {
        let ventende = self
            .execution_repo
            .hent_ventende_for_journalpost(journalpost_id)
            .await?;
        self.reevaluer(ventende).await
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
        let registration_model = registration.til_eksekveringssystem_registrering();

        match self
            .klarhet_vurderer
            .vurder(
                self.snapshot_repo.as_ref(),
                &registration_model,
                &kommando.envelope,
                registration.sak_id(),
                registration.journalpost_id(),
            )
            .await
            .with_context(|| {
                format!(
                    "Klarte ikke vurdere klarhet for ventende command {}",
                    kommando.command_id
                )
            })? {
            Kjorbarhet::Klar => {
                self.execution_repo
                    .oppdater_til_klar(kommando.command_id)
                    .await?;
            }
            Kjorbarhet::Venter { grunn, detalj } => {
                self.execution_repo
                    .oppdater_venter(kommando.command_id, &grunn, &detalj)
                    .await?;
            }
            Kjorbarhet::Feil { detalj } => {
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
