use crate::command::{Command, CommandEnvelope};
use domain::eksekvering::typer::{
    CommandLifecycleContext, CommandLifecycleEvent, EksekveringFeil, EksekveringFeiltype,
    StatusErrorCode,
};

use crate::command::status::{
    utfores_blocked_event, utfores_error_event, utfores_ok_event, utfores_retrying_event,
};

use super::{EksekverKommandoService, ExecutionOutcome, safe_execution_detail};

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

    pub(super) async fn publish_blocked_with_detail(
        &self,
        envelope: &CommandEnvelope<Command>,
        attempt: u32,
        detail: String,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let context = self.resolve_execution_context(envelope).await?;
        let safe_detail = safe_execution_detail(&detail);
        let event = utfores_blocked_event(
            envelope,
            &safe_detail,
            Some(StatusErrorCode::PrerequisitePending),
            context,
            Some(attempt),
        );
        self.publish_status(event, envelope).await?;
        Ok(ExecutionOutcome::BlokkertVenter {
            last_error: Some(safe_detail),
        })
    }

    pub(super) async fn map_feil_til_outcome(
        &self,
        envelope: &CommandEnvelope<Command>,
        err: EksekveringFeil,
        attempt: u32,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let context = self.resolve_execution_context(envelope).await?;

        let safe_detail = safe_execution_detail(&err.melding);

        match err.feiltype {
            EksekveringFeiltype::Recoverable => {
                let event = utfores_retrying_event(
                    envelope,
                    &safe_detail,
                    Some(StatusErrorCode::TemporaryUnavailable),
                    context,
                    Some(attempt),
                );
                self.publish_status(event, envelope).await?;
                Ok(ExecutionOutcome::Retrying {
                    last_error: Some(safe_detail),
                })
            }
            EksekveringFeiltype::Irrecoverable => {
                let event = utfores_error_event(
                    envelope,
                    &safe_detail,
                    Some(StatusErrorCode::ProcessingFailed),
                    context,
                    Some(attempt),
                );
                self.publish_status(event, envelope).await?;
                Ok(ExecutionOutcome::Feil {
                    last_error: Some(safe_detail),
                })
            }
            EksekveringFeiltype::Blocked => {
                let event = utfores_blocked_event(
                    envelope,
                    &safe_detail,
                    Some(StatusErrorCode::PrerequisitePending),
                    context,
                    Some(attempt),
                );
                self.publish_status(event, envelope).await?;
                Ok(ExecutionOutcome::BlokkertVenter {
                    last_error: Some(safe_detail),
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
            self.done_publisher.publiser_done(envelope).await?;
        }

        Ok(())
    }
}
