mod dokument_handlers;
mod execution_report;
mod journalpost_handlers;
mod lifecycle_publisher;
mod plan_resolver;
mod prerequisite;
mod resolved_plan;
mod sak_handlers;
mod state_reader;
mod step_outcome;
mod wakeup;

use domain::eksekvering::execution::Ventegrunn;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

use crate::command::ports::eksekvering_port::{
    ArkivGateway, EksekveringKvitteringPublisher, EksekveringStatusPublisher,
};
use crate::command::ports::execution_snapshot_port::EksekveringSnapshotRepository;
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::ports::ventende_kommando_wakeup_port::VentendeKommandoWakeup;
use domain::eksekvering::plan::EksekveringsPlan;
use domain::eksekvering::typer::EksekveringFeil;

use self::execution_report::ExecutionReport;
use self::resolved_plan::{ResolvedPlan, ResolvedStep};
use self::step_outcome::StepOutcome;

pub struct EksekverKommandoService {
    snapshot_repo: Box<dyn EksekveringSnapshotRepository>,
    arkiv_gateway: Box<dyn ArkivGateway>,
    status_publisher: Box<dyn EksekveringStatusPublisher>,
    done_publisher: Box<dyn EksekveringKvitteringPublisher>,
    id_mapping: Box<dyn IdMappingRepository>,
    outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
    wakeup_service: Box<dyn VentendeKommandoWakeup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionOutcome {
    Ok,
    Blocked {
        grunn: Option<Ventegrunn>,
        last_error: Option<String>,
    },
    Retrying {
        last_error: Option<String>,
    },
    Error {
        last_error: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct ExecutionFailure {
    err: EksekveringFeil,
    report: ExecutionReport,
}

impl ExecutionFailure {
    fn new(err: EksekveringFeil, report: ExecutionReport) -> Self {
        Self { err, report }
    }
}

impl EksekverKommandoService {
    pub fn new(
        snapshot_repo: Box<dyn EksekveringSnapshotRepository>,
        arkiv_gateway: Box<dyn ArkivGateway>,
        status_publisher: Box<dyn EksekveringStatusPublisher>,
        done_publisher: Box<dyn EksekveringKvitteringPublisher>,
        id_mapping: Box<dyn IdMappingRepository>,
        outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
        wakeup_service: Box<dyn VentendeKommandoWakeup>,
    ) -> Self {
        Self {
            snapshot_repo,
            arkiv_gateway,
            status_publisher,
            done_publisher,
            id_mapping,
            outward_status_projector,
            wakeup_service,
        }
    }

    pub async fn handle(
        &self,
        envelope: CommandEnvelope<Command>,
        attempt: u32,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let plan = match EksekveringsPlan::fra_command(&envelope.payload) {
            Ok(plan) => plan,
            Err(err) => {
                return self
                    .avslutt_med_feil(&envelope, err, attempt, ExecutionReport::default())
                    .await;
            }
        };

        let resolved_plan = match self.resolve_plan(&envelope, plan).await {
            Ok(plan) => plan,
            Err(err) => {
                return self
                    .avslutt_med_feil(&envelope, err, attempt, ExecutionReport::default())
                    .await;
            }
        };

        match self.execute_plan(&envelope, resolved_plan).await {
            Ok(report) => self.publish_success(&envelope, attempt, report).await,
            Err(failure) => {
                self.avslutt_med_feil(&envelope, failure.err, attempt, failure.report)
                    .await
            }
        }
    }

    async fn execute_plan(
        &self,
        envelope: &CommandEnvelope<Command>,
        plan: ResolvedPlan,
    ) -> Result<ExecutionReport, ExecutionFailure> {
        let mut report = ExecutionReport::default();

        for steg in plan.steg {
            let outcome = self
                .execute_steg(envelope, steg, &mut report)
                .await
                .map_err(|err| ExecutionFailure::new(err, report.clone()))?;

            if let StepOutcome::Blocked {
                prerequisite,
                detail,
            } = outcome
            {
                report.block(prerequisite, detail.clone());
                return Err(ExecutionFailure::new(
                    EksekveringFeil::blocked(detail),
                    report,
                ));
            }
        }

        Ok(report)
    }

    async fn execute_steg(
        &self,
        envelope: &CommandEnvelope<Command>,
        steg: ResolvedStep,
        report: &mut ExecutionReport,
    ) -> Result<StepOutcome, EksekveringFeil> {
        match steg {
            ResolvedStep::OpprettSak {
                sak_id,
                sak_client_reference,
            } => {
                self.opprett_sak(envelope, sak_id, sak_client_reference, report)
                    .await
            }
            ResolvedStep::OpprettJournalpost { plan } => {
                self.opprett_journalpost(envelope, plan, report).await
            }
            ResolvedStep::LeggTilDokument {
                journalpost_id,
                dokument_id,
                dokument_client_reference,
            } => {
                self.legg_til_dokument(
                    envelope,
                    journalpost_id,
                    dokument_id,
                    dokument_client_reference,
                    report,
                )
                .await
            }
            ResolvedStep::Journalfoer { journalpost_id } => {
                self.journalfoer_journalpost(envelope, journalpost_id).await
            }
            ResolvedStep::Avskriv { journalpost_id } => {
                self.avskriv_journalpost(envelope, journalpost_id).await
            }
            ResolvedStep::AvsluttSak { sak_id } => self.avslutt_sak(envelope, sak_id).await,
        }
    }

    fn map_arkiv_feil(&self, err: anyhow::Error) -> EksekveringFeil {
        let original = err.to_string();
        let message = original
            .replace("sikri_recoverability=irrecoverable", "")
            .replace("sikri_recoverability=recoverable", "")
            .trim()
            .to_string();

        if original.contains("sikri_recoverability=irrecoverable") {
            return EksekveringFeil::irrecoverable(message);
        }

        EksekveringFeil::recoverable(message)
    }
}
