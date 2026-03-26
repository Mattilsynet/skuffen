use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::regler::kan_avslutte_sak;
use domain::eksekvering::typer::EksekveringFeil;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use uuid::Uuid;

use crate::command::ports::execution_snapshot_port::{SakStatus, SakTransition};

use super::execution_report::ExecutionReport;
use super::prerequisite::Prerequisite;
use super::state_reader::til_journalpost_rule_state;
use super::step_outcome::StepOutcome;
use super::EksekverKommandoService;

impl EksekverKommandoService {
    pub(super) async fn opprett_sak(
        &self,
        envelope: &CommandEnvelope<Command>,
        sak_id: SkuffenSakId,
        sak_client_reference: Uuid,
        report: &mut ExecutionReport,
    ) -> Result<StepOutcome, EksekveringFeil> {
        if self
            .hent_sak_state(sak_id)
            .await?
            .is_some_and(|existing| existing.opprettet)
        {
            let outcome = StepOutcome::AlreadyCompleted;
            self.maybe_wake_after_sak_step(sak_id, &outcome).await?;
            return Ok(outcome);
        }

        let saksnummer = self
            .arkiv_gateway
            .opprett_sak(envelope)
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        self.id_mapping
            .oppdater_arkiv_id_for_client_reference(sak_client_reference, saksnummer.clone())
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        self.snapshot_repo
            .anvend_sak_transition(
                sak_id,
                SakTransition {
                    status: SakStatus::UnderBehandling,
                    opprettet: true,
                    saksnummer: Some(saksnummer.clone()),
                },
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        report.set_saksnummer(saksnummer);

        let outcome = StepOutcome::Completed;
        self.maybe_wake_after_sak_step(sak_id, &outcome).await?;
        Ok(outcome)
    }

    pub(super) async fn avslutt_sak(
        &self,
        _envelope: &CommandEnvelope<Command>,
        sak_id: SkuffenSakId,
    ) -> Result<StepOutcome, EksekveringFeil> {
        let Some(state) = self.hent_sak_state(sak_id).await? else {
            return Ok(StepOutcome::blocked(
                Some(Prerequisite::SakOpprettet { sak_id }),
                "Kan ikke avslutte sak: saken finnes ikke i state",
            ));
        };

        if state.status == SakStatus::Avsluttet {
            let outcome = StepOutcome::AlreadyCompleted;
            self.maybe_wake_after_sak_step(sak_id, &outcome).await?;
            return Ok(outcome);
        }

        let journalposter = self.hent_journalposter_for_sak(sak_id).await?;
        let journalposter = journalposter
            .iter()
            .map(til_journalpost_rule_state)
            .collect::<Vec<_>>();
        kan_avslutte_sak(&journalposter)?;

        let saksnummer = self.hent_saksnummer_for_sak(sak_id).await?.ok_or_else(|| {
            EksekveringFeil::blocked("Kan ikke avslutte sak: saksnummer mangler i state")
        })?;

        self.arkiv_gateway
            .avslutt_sak(saksnummer.as_str())
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        self.snapshot_repo
            .anvend_sak_transition(
                sak_id,
                SakTransition {
                    status: SakStatus::Avsluttet,
                    opprettet: true,
                    saksnummer: Some(saksnummer),
                },
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        let outcome = StepOutcome::Completed;
        self.maybe_wake_after_sak_step(sak_id, &outcome).await?;
        Ok(outcome)
    }
}
