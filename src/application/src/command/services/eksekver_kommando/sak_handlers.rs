use domain::eksekvering::regler::kan_avslutte_sak;
use domain::eksekvering::typer::EksekveringFeil;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use uuid::Uuid;

use crate::command::ports::eksekvering_state_port::{SakState, SakStatus};

use super::execution_report::ExecutionReport;
use super::prerequisite::Prerequisite;
use super::state_reader::til_journalpost_rule_state;
use super::step_outcome::StepOutcome;
use super::EksekverKommandoService;

impl EksekverKommandoService {
    pub(super) async fn opprett_sak(
        &self,
        envelope: &CommandEnvelope<Command>,
        sak_id: Uuid,
        sak_client_reference: Uuid,
        report: &mut ExecutionReport,
    ) -> Result<StepOutcome, EksekveringFeil> {
        if self
            .hent_sak_state(sak_id)
            .await?
            .is_some_and(|existing| existing.opprettet)
        {
            return Ok(StepOutcome::AlreadyCompleted);
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

        self.state_repo
            .lagre_sak_state(
                sak_id,
                SakState {
                    status: SakStatus::UnderBehandling,
                    opprettet: true,
                    saksnummer: Some(saksnummer.clone()),
                },
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        report.set_saksnummer(saksnummer);

        Ok(StepOutcome::Completed)
    }

    pub(super) async fn avslutt_sak(
        &self,
        _envelope: &CommandEnvelope<Command>,
        sak_id: Uuid,
    ) -> Result<StepOutcome, EksekveringFeil> {
        let Some(state) = self.hent_sak_state(sak_id).await? else {
            return Ok(StepOutcome::blocked(
                Some(Prerequisite::SakOpprettet { sak_id }),
                "Kan ikke avslutte sak: saken finnes ikke i state",
            ));
        };

        if state.status == SakStatus::Avsluttet {
            return Ok(StepOutcome::AlreadyCompleted);
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

        self.state_repo
            .lagre_sak_state(
                sak_id,
                SakState {
                    status: SakStatus::Avsluttet,
                    opprettet: true,
                    saksnummer: Some(saksnummer),
                },
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(StepOutcome::Completed)
    }
}
