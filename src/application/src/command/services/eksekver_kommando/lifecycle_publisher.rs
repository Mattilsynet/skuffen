use domain::eksekvering::typer::{
    CommandLifecycleContext, CommandLifecycleEvent, EksekveringFeil, EksekveringFeiltype,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::status::SkuffenStatusErrorCode;

use crate::command::lifecycle::LifecycleDecision;
use crate::command::status::{
    utfores_blocked_event, utfores_error_event, utfores_ok_event, utfores_retrying_event,
};

use super::execution_report::ExecutionReport;
use super::{EksekverKommandoService, ExecutionOutcome};

impl EksekverKommandoService {
    fn execution_lifecycle_decision(
        &self,
        err: &EksekveringFeil,
        report: &ExecutionReport,
    ) -> LifecycleDecision {
        let detail = report.detail.clone().unwrap_or_else(|| err.melding.clone());

        match err.feiltype {
            EksekveringFeiltype::Recoverable => LifecycleDecision::retrying(
                detail,
                Some(SkuffenStatusErrorCode::TemporaryUnavailable),
            ),
            EksekveringFeiltype::Irrecoverable => {
                LifecycleDecision::error(detail, Some(SkuffenStatusErrorCode::ProcessingFailed))
            }
            EksekveringFeiltype::Blocked => LifecycleDecision::blocked(
                detail,
                report
                    .blocked_by
                    .as_ref()
                    .map(|prerequisite| prerequisite.as_error_code()),
            ),
        }
    }

    async fn resolve_execution_context(
        &self,
        envelope: &CommandEnvelope<Command>,
        report: &ExecutionReport,
    ) -> Result<CommandLifecycleContext, anyhow::Error> {
        let report_context = report.clone().into_context();
        if !report_context.is_empty() {
            let projected_context = self
                .outward_status_projector
                .resolve_context(envelope)
                .await?;

            return Ok(report.clone().merge_context_over(projected_context));
        }

        let projected_context = self
            .outward_status_projector
            .resolve_context(envelope)
            .await?;

        Ok(projected_context)
    }

    pub(super) async fn publish_success(
        &self,
        envelope: &CommandEnvelope<Command>,
        attempt: u32,
        report: ExecutionReport,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let context = self.resolve_execution_context(envelope, &report).await?;
        let decision = LifecycleDecision::ok(report.detail.clone());
        let event = utfores_ok_event(envelope, decision.detail, context, Some(attempt));
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
        let context = self.resolve_execution_context(envelope, &report).await?;
        let decision = self.execution_lifecycle_decision(&err, &report);

        let event = match decision.stage_status {
            domain::eksekvering::typer::CommandStageStatus::Retrying => utfores_retrying_event(
                envelope,
                decision
                    .detail
                    .clone()
                    .unwrap_or_else(|| err.melding.clone()),
                decision.error_code.clone(),
                context,
                Some(attempt),
            ),
            domain::eksekvering::typer::CommandStageStatus::Error => utfores_error_event(
                envelope,
                decision
                    .detail
                    .clone()
                    .unwrap_or_else(|| err.melding.clone()),
                decision.error_code.clone(),
                context,
                Some(attempt),
            ),
            domain::eksekvering::typer::CommandStageStatus::Blocked => utfores_blocked_event(
                envelope,
                decision
                    .detail
                    .clone()
                    .unwrap_or_else(|| err.melding.clone()),
                decision.error_code.clone(),
                context,
                Some(attempt),
            ),
            domain::eksekvering::typer::CommandStageStatus::Ok
            | domain::eksekvering::typer::CommandStageStatus::Venter => {
                unreachable!("execution failure must map to non-ok lifecycle decision")
            }
        };
        self.publish_status(event, envelope).await?;

        let detail = decision.detail.unwrap_or_else(|| err.melding.clone());

        Ok(match decision.stage_status {
            domain::eksekvering::typer::CommandStageStatus::Retrying => {
                ExecutionOutcome::Retrying {
                    last_error: Some(detail),
                }
            }
            domain::eksekvering::typer::CommandStageStatus::Error => ExecutionOutcome::Error {
                last_error: Some(detail),
            },
            domain::eksekvering::typer::CommandStageStatus::Blocked => ExecutionOutcome::Blocked {
                last_error: Some(detail),
            },
            domain::eksekvering::typer::CommandStageStatus::Ok
            | domain::eksekvering::typer::CommandStageStatus::Venter => {
                unreachable!("execution failure must map to non-ok lifecycle outcome")
            }
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
