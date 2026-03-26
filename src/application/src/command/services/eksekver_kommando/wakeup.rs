use domain::eksekvering::id::{SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::typer::EksekveringFeil;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

use super::resolved_plan::ResolvedJournalpostPlan;
use super::step_outcome::StepOutcome;
use super::EksekverKommandoService;

impl EksekverKommandoService {
    pub(super) async fn wake_sak(&self, sak_id: SkuffenSakId) -> Result<(), EksekveringFeil> {
        self.wakeup_service
            .etter_sak_endret(sak_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))
    }

    pub(super) async fn wake_journalpost(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<(), EksekveringFeil> {
        self.wakeup_service
            .etter_journalpost_endret(journalpost_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))
    }

    pub(super) async fn wake_journalpost_and_sak(
        &self,
        journalpost_id: SkuffenJournalpostId,
        sak_id: SkuffenSakId,
    ) -> Result<(), EksekveringFeil> {
        self.wake_journalpost(journalpost_id).await?;
        self.wake_sak(sak_id).await
    }

    pub(super) async fn wake_sak_for_journalpost_envelope(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<(), EksekveringFeil> {
        let sak_id = self
            .resolve_sak_id_for_journalpost_envelope(envelope)
            .await?;
        self.wake_sak(sak_id).await
    }

    pub(super) async fn maybe_wake_after_journalpost_step(
        &self,
        envelope: &CommandEnvelope<Command>,
        journalpost_id: SkuffenJournalpostId,
        outcome: &StepOutcome,
    ) -> Result<(), EksekveringFeil> {
        if matches!(
            outcome,
            StepOutcome::Completed | StepOutcome::AlreadyCompleted
        ) {
            self.wake_journalpost(journalpost_id).await?;
            self.wake_sak_for_journalpost_envelope(envelope).await?;
        }

        Ok(())
    }

    pub(super) async fn maybe_wake_after_sak_step(
        &self,
        sak_id: SkuffenSakId,
        outcome: &StepOutcome,
    ) -> Result<(), EksekveringFeil> {
        if matches!(
            outcome,
            StepOutcome::Completed | StepOutcome::AlreadyCompleted
        ) {
            self.wake_sak(sak_id).await?;
        }

        Ok(())
    }

    pub(super) async fn maybe_wake_after_opprett_journalpost(
        &self,
        plan: &ResolvedJournalpostPlan,
        outcome: &StepOutcome,
    ) -> Result<(), EksekveringFeil> {
        if matches!(
            outcome,
            StepOutcome::Completed | StepOutcome::AlreadyCompleted
        ) {
            self.wake_journalpost_and_sak(plan.journalpost_id, plan.sak_id)
                .await?;
        }

        Ok(())
    }

    pub(super) async fn maybe_wake_after_dokument_step(
        &self,
        envelope: &CommandEnvelope<Command>,
        journalpost_id: SkuffenJournalpostId,
        outcome: &StepOutcome,
    ) -> Result<(), EksekveringFeil> {
        if matches!(
            outcome,
            StepOutcome::Completed | StepOutcome::AlreadyCompleted
        ) {
            self.wake_journalpost(journalpost_id).await?;
            self.wake_sak_for_journalpost_envelope(envelope).await?;
        }

        Ok(())
    }
}
