mod resolved_registration;

use anyhow::Result;
use async_trait::async_trait;
use domain::eksekvering::typer::CommandLifecycleEvent;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

use crate::command::ports::eksekvering_port::EksekveringStatusPublisher;
use crate::command::ports::eksekvering_state_port::{
    EksekveringStateRepository, EksekveringsregistreringResultat,
};
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::registrer_i_eksekveringssystem_port::RegistrerIEksekveringssystemUseCase;
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::status::utfores_venter_event;

use self::resolved_registration::resolve_registration;

/// Tynn registrering inn i eksekveringssystemet for kommandoer som allerede er validert.
///
/// Denne servicen skal ikke gjore ekstra forretningsvalidering. Den kan lese
/// kommandoshape og lese/opprette identiteter i `id_mapping` for a materialisere
/// eksekveringssystemets state,
/// registrere kommandoen i `command_execution`, og publisere `utfores::venter`
/// nar registreringen faktisk ble opprettet.
pub struct RegistrerIEksekveringssystemService {
    state_repo: Box<dyn EksekveringStateRepository>,
    id_mapping_repo: Box<dyn IdMappingRepository>,
    status_publisher: Box<dyn EksekveringStatusPublisher>,
    outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
}

impl RegistrerIEksekveringssystemService {
    pub fn new(
        state_repo: Box<dyn EksekveringStateRepository>,
        id_mapping_repo: Box<dyn IdMappingRepository>,
        status_publisher: Box<dyn EksekveringStatusPublisher>,
        outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
    ) -> Self {
        Self {
            state_repo,
            id_mapping_repo,
            status_publisher,
            outward_status_projector,
        }
    }

    async fn ensure_registrert_i_eksekveringssystem(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<EksekveringsregistreringResultat> {
        let registration =
            resolve_registration(self.id_mapping_repo.as_ref(), &envelope.payload).await?;
        self.state_repo
            .ensure_registrert_i_eksekveringssystem(
                &registration.til_eksekveringssystem_registrering(),
                envelope,
            )
            .await
    }

    async fn emit_status(&self, event: CommandLifecycleEvent) -> Result<()> {
        self.status_publisher.publiser_status(event).await
    }
}

#[async_trait]
impl RegistrerIEksekveringssystemUseCase for RegistrerIEksekveringssystemService {
    async fn handle(&self, envelope: &CommandEnvelope<Command>) -> Result<()> {
        let registrering = self
            .ensure_registrert_i_eksekveringssystem(envelope)
            .await?;

        if registrering.skal_publisere_utfores_venter() {
            let context = self
                .outward_status_projector
                .resolve_context(envelope)
                .await?;
            self.emit_status(utfores_venter_event(envelope, context, Some(1)))
                .await?;
            self.state_repo
                .marker_utfores_venter_publisert(envelope.command_id)
                .await?;
        }

        Ok(())
    }
}
