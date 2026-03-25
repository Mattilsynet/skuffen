use domain::eksekvering::typer::{CommandLifecycleEvent, EksekveringFeil, EksekveringFeiltype};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

use crate::command::status::{
    utfores_blocked_event, utfores_error_event, utfores_ok_event, utfores_retrying_event,
};

use super::execution_report::ExecutionReport;
use super::{EksekverKommandoService, ExecutionOutcome};

impl EksekverKommandoService {
    pub(super) async fn publish_success(
        &self,
        envelope: &CommandEnvelope<Command>,
        attempt: u32,
        report: ExecutionReport,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let context = self
            .status_context_resolver
            .resolve_context(envelope)
            .await?;
        let event = utfores_ok_event(envelope, report.detail, context, Some(attempt));
        self.publish_status(event, envelope).await?;
        Ok(ExecutionOutcome::Ok)
    }

    pub(super) async fn avslutt_med_feil(
        &self,
        envelope: &CommandEnvelope<Command>,
        err: EksekveringFeil,
        attempt: u32,
        report: ExecutionReport,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let context = self
            .status_context_resolver
            .resolve_context(envelope)
            .await?;
        let detail = report.detail.unwrap_or_else(|| err.melding.clone());

        let event = match err.feiltype {
            EksekveringFeiltype::Recoverable => {
                utfores_retrying_event(envelope, detail.clone(), context, Some(attempt))
            }
            EksekveringFeiltype::Irrecoverable => {
                utfores_error_event(envelope, detail.clone(), context, Some(attempt))
            }
            EksekveringFeiltype::Blocked => {
                utfores_blocked_event(envelope, detail.clone(), context, Some(attempt))
            }
        };
        self.publish_status(event, envelope).await?;

        Ok(match err.feiltype {
            EksekveringFeiltype::Recoverable => ExecutionOutcome::Retrying {
                last_error: Some(detail),
            },
            EksekveringFeiltype::Irrecoverable => ExecutionOutcome::Error {
                last_error: Some(detail),
            },
            EksekveringFeiltype::Blocked => ExecutionOutcome::Blocked {
                last_error: Some(detail),
            },
        })
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
