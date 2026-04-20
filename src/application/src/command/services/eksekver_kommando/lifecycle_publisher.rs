use domain::eksekvering::typer::{
    CommandLifecycleContext, CommandLifecycleEvent, EksekveringFeil, EksekveringFeiltype,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::status::SkuffenStatusErrorCode;

use crate::command::status::{
    utfores_blocked_event, utfores_error_event, utfores_ok_event, utfores_retrying_event,
};

use super::{EksekverKommandoService, ExecutionOutcome};

impl EksekverKommandoService {
    pub(super) async fn publish_success(
        &self,
        envelope: &CommandEnvelope<Command>,
        attempt: u32,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let context = self.resolve_execution_context(envelope).await?;
        let event = utfores_ok_event(envelope, None, context, Some(attempt));
        self.publish_status(event, envelope).await?;
        Ok(ExecutionOutcome::Ok)
    }

    pub(super) async fn publish_blocked(
        &self,
        envelope: &CommandEnvelope<Command>,
        attempt: u32,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let context = self.resolve_execution_context(envelope).await?;
        let detail = "Kommando venter på at prerequisite fullføres".to_string();
        let event = utfores_blocked_event(
            envelope,
            &detail,
            Some(SkuffenStatusErrorCode::PrerequisitePending),
            context,
            Some(attempt),
        );
        self.publish_status(event, envelope).await?;
        Ok(ExecutionOutcome::BlokkertVenter {
            last_error: Some(detail),
        })
    }

    pub(super) async fn map_feil_til_outcome(
        &self,
        envelope: &CommandEnvelope<Command>,
        err: EksekveringFeil,
        attempt: u32,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let context = self.resolve_execution_context(envelope).await?;

        match err.feiltype {
            EksekveringFeiltype::Recoverable => {
                let event = utfores_retrying_event(
                    envelope,
                    &err.melding,
                    Some(SkuffenStatusErrorCode::TemporaryUnavailable),
                    context,
                    Some(attempt),
                );
                self.publish_status(event, envelope).await?;
                Ok(ExecutionOutcome::Retrying {
                    last_error: Some(err.melding),
                })
            }
            EksekveringFeiltype::Irrecoverable => {
                let event = utfores_error_event(
                    envelope,
                    &err.melding,
                    Some(SkuffenStatusErrorCode::ProcessingFailed),
                    context,
                    Some(attempt),
                );
                self.publish_status(event, envelope).await?;
                Ok(ExecutionOutcome::Feil {
                    last_error: Some(err.melding),
                })
            }
            EksekveringFeiltype::Blocked => {
                let event = utfores_blocked_event(
                    envelope,
                    &err.melding,
                    Some(SkuffenStatusErrorCode::PrerequisitePending),
                    context,
                    Some(attempt),
                );
                self.publish_status(event, envelope).await?;
                Ok(ExecutionOutcome::BlokkertVenter {
                    last_error: Some(err.melding),
                })
            }
        }
    }

    async fn resolve_execution_context(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<CommandLifecycleContext, anyhow::Error> {
        self.outward_status_projector
            .resolve_context(envelope)
            .await
    }

    async fn publish_status(
        &self,
        event: CommandLifecycleEvent,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error> {
        let terminal = event.terminal;
        self.status_publisher.publiser_status(event).await?;

        if terminal {
            let (subject, _) = domain::eksekvering::typer::done_subject(envelope);
            self.done_publisher
                .publiser_done(&subject, envelope)
                .await?;
        }

        Ok(())
    }
}
