use crate::command::services::execution_registration::resolve_registration;

use anyhow::Result;
use async_trait::async_trait;
use domain::eksekvering::execution::{EksekveringStatus, Kjorbarhet};
use domain::eksekvering::typer::{command_metadata, CommandLifecycleEvent};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

use crate::command::ports::command_execution_port::{
    CommandExecutionRepository, EksekveringsregistreringResultat, NyKommandoEksekvering,
};
use crate::command::ports::eksekvering_port::EksekveringStatusPublisher;
use crate::command::ports::execution_snapshot_port::EksekveringSnapshotRepository;
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::registrer_i_eksekveringssystem_port::RegistrerIEksekveringssystemUseCase;
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::services::eksekveringsklarhet_vurderer::EksekveringsklarhetVurderer;
use crate::command::status::{utfores_error_event, utfores_venter_event};

/// Tynn registrering inn i eksekveringssystemet for kommandoer som allerede er validert.
///
/// Denne servicen skal ikke gjore ekstra forretningsvalidering. Den kan lese
/// kommandoshape og lese/opprette identiteter i `id_mapping` for a materialisere
/// eksekveringssystemets state,
/// registrere kommandoen i `command_execution`, og publisere `utfores::venter`
/// nar registreringen faktisk ble opprettet.
pub struct RegistrerIEksekveringssystemService {
    execution_repo: Box<dyn CommandExecutionRepository>,
    snapshot_repo: Box<dyn EksekveringSnapshotRepository>,
    id_mapping_repo: Box<dyn IdMappingRepository>,
    status_publisher: Box<dyn EksekveringStatusPublisher>,
    outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
}

impl RegistrerIEksekveringssystemService {
    pub fn new(
        execution_repo: Box<dyn CommandExecutionRepository>,
        snapshot_repo: Box<dyn EksekveringSnapshotRepository>,
        id_mapping_repo: Box<dyn IdMappingRepository>,
        status_publisher: Box<dyn EksekveringStatusPublisher>,
        outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
    ) -> Self {
        Self {
            execution_repo,
            snapshot_repo,
            id_mapping_repo,
            status_publisher,
            outward_status_projector,
        }
    }

    async fn ensure_registrert_i_eksekveringssystem(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<(EksekveringsregistreringResultat, Kjorbarhet)> {
        let registration = resolve_registration(self.id_mapping_repo.as_ref(), envelope).await?;
        let registration_model = registration.til_eksekveringssystem_registrering();
        let klarhet = EksekveringsklarhetVurderer::new()
            .vurder(
                self.snapshot_repo.as_ref(),
                &registration_model,
                envelope,
                registration.sak_id(),
                registration.journalpost_id(),
            )
            .await?;

        let ny = NyKommandoEksekvering {
            envelope: envelope.clone(),
            command_type: command_metadata(&envelope.payload).0,
            sak_id: registration.sak_id(),
            journalpost_id: registration.journalpost_id(),
            status: match &klarhet {
                Kjorbarhet::Klar => EksekveringStatus::Klar,
                Kjorbarhet::Venter { .. } => EksekveringStatus::Venter,
                Kjorbarhet::Feil { .. } => EksekveringStatus::Feil,
            },
            ventegrunn: match &klarhet {
                Kjorbarhet::Venter { grunn, .. } => Some(grunn.clone()),
                _ => None,
            },
            last_detail: match &klarhet {
                Kjorbarhet::Venter { detalj, .. } | Kjorbarhet::Feil { detalj } => {
                    Some(detalj.clone())
                }
                Kjorbarhet::Klar => None,
            },
        };

        let resultat = self.execution_repo.opprett(&registration_model, ny).await?;

        Ok((resultat, klarhet))
    }

    async fn emit_status(&self, event: CommandLifecycleEvent) -> Result<()> {
        self.status_publisher.publiser_status(event).await
    }
}

#[async_trait]
impl RegistrerIEksekveringssystemUseCase for RegistrerIEksekveringssystemService {
    async fn handle(&self, envelope: &CommandEnvelope<Command>) -> Result<()> {
        let (registrering, klarhet) = self
            .ensure_registrert_i_eksekveringssystem(envelope)
            .await?;

        let context = self
            .outward_status_projector
            .resolve_context(envelope)
            .await?;

        match klarhet {
            Kjorbarhet::Feil { detalj } => {
                if matches!(registrering, EksekveringsregistreringResultat::Nyregistrert) {
                    self.emit_status(utfores_error_event(
                        envelope,
                        detalj,
                        None,
                        context,
                        Some(1),
                    ))
                    .await?;
                }
            }
            Kjorbarhet::Klar | Kjorbarhet::Venter { .. } => {
                if registrering.skal_publisere_utfores_venter() {
                    self.emit_status(utfores_venter_event(envelope, context, Some(1)))
                        .await?;
                    self.execution_repo
                        .marker_utfores_venter_publisert(envelope.command_id)
                        .await?;
                }
            }
        }

        Ok(())
    }
}
