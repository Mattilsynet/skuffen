use domain::eksekvering::id::SkuffenJournalpostId;
use domain::eksekvering::regler::{
    kan_avskrive_journalpost, kan_journalfoere_journalpost, kan_opprette_journalpost_pa_sak,
    neste_journalpost_status_ved_journalfoering,
};
use domain::eksekvering::typer::EksekveringFeil;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

use crate::command::ports::eksekvering_port::{OpprettJournalpostResultat, Utsendingsvalg};
use crate::command::ports::eksekvering_state_port::{
    JournalpostOpprettetTransition, JournalpostOvergangVedJournalfoering,
};

use super::execution_report::ExecutionReport;
use super::prerequisite::Prerequisite;
use super::resolved_plan::ResolvedJournalpostPlan;
use super::state_reader::{til_journalpost_rule_state, til_sak_rule_state};
use super::step_outcome::StepOutcome;
use super::EksekverKommandoService;

impl EksekverKommandoService {
    pub(super) async fn opprett_journalpost(
        &self,
        envelope: &CommandEnvelope<Command>,
        plan: ResolvedJournalpostPlan,
        report: &mut ExecutionReport,
    ) -> Result<StepOutcome, EksekveringFeil> {
        let Some(sak_state) = self.hent_sak_state(plan.sak_id).await? else {
            return Ok(StepOutcome::blocked(
                Some(Prerequisite::SakOpprettet {
                    sak_id: plan.sak_id,
                }),
                "Sak finnes ikke i skuffen-state",
            ));
        };
        kan_opprette_journalpost_pa_sak(&til_sak_rule_state(&sak_state))?;

        if self
            .hent_journalpost_state(plan.journalpost_id)
            .await?
            .and_then(|state| state.journalpostnummer)
            .is_some()
        {
            return Ok(StepOutcome::AlreadyCompleted);
        }

        let utsending = plan.utsending.map(|utsending| match utsending {
            domain::eksekvering::plan::Utsending::MedUtsending => Utsendingsvalg::MedUtsending,
            domain::eksekvering::plan::Utsending::UtenUtsending => Utsendingsvalg::UtenUtsending,
        });

        let saksnummer = match self.hent_saksnummer_for_sak(plan.sak_id).await? {
            Some(saksnummer) => saksnummer,
            None => {
                return Ok(StepOutcome::blocked(
                    Some(Prerequisite::SaksnummerTildelt {
                        sak_id: plan.sak_id,
                    }),
                    "Saksnummer mangler",
                ))
            }
        };

        report.set_saksnummer(saksnummer.clone());

        let OpprettJournalpostResultat { journalpost_id } = self
            .arkiv_gateway
            .opprett_journalpost(envelope, saksnummer.as_str(), utsending)
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        self.id_mapping
            .oppdater_arkiv_id_for_client_reference(
                plan.journalpost_client_reference,
                journalpost_id.to_string(),
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        self.state_repo
            .anvend_journalpost_opprettet(
                plan.journalpost_id,
                JournalpostOpprettetTransition {
                    journalpostnummer: journalpost_id,
                },
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        if let Some(hoveddokument) = plan.dokumenter.first() {
            self.state_repo
                .anvend_dokument_lagt_til(hoveddokument.dokument_id, plan.journalpost_id)
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        }

        report.set_journalpost_id(journalpost_id.to_string());

        Ok(StepOutcome::Completed)
    }

    pub(super) async fn journalfoer_journalpost(
        &self,
        _envelope: &CommandEnvelope<Command>,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<StepOutcome, EksekveringFeil> {
        let Some(state) = self.hent_journalpost_state(journalpost_id).await? else {
            return Ok(StepOutcome::blocked(
                Some(Prerequisite::JournalpostOpprettet { journalpost_id }),
                "Kan ikke journalfore journalpost: journalpost finnes ikke i state",
            ));
        };

        if state.journalfoert {
            return Ok(StepOutcome::AlreadyCompleted);
        }

        kan_journalfoere_journalpost(&til_journalpost_rule_state(&state))?;

        let journalpostnummer = match state.journalpostnummer {
            Some(journalpostnummer) => journalpostnummer,
            None => {
                return Ok(StepOutcome::blocked(
                    Some(Prerequisite::JournalpostnummerTildelt { journalpost_id }),
                    "Journalpostnummer mangler",
                ))
            }
        };

        let transition =
            neste_journalpost_status_ved_journalfoering(&til_journalpost_rule_state(&state));

        self.arkiv_gateway
            .sett_journalpost_status(journalpostnummer, transition.ny_status)
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        self.state_repo
            .anvend_journalpost_overgang_ved_journalfoering(
                journalpost_id,
                JournalpostOvergangVedJournalfoering {
                    journalfoert: transition.journalfoert,
                    ekspedert: transition.ekspedert,
                },
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(StepOutcome::Completed)
    }

    pub(super) async fn avskriv_journalpost(
        &self,
        _envelope: &CommandEnvelope<Command>,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<StepOutcome, EksekveringFeil> {
        let Some(state) = self.hent_journalpost_state(journalpost_id).await? else {
            return Ok(StepOutcome::blocked(
                Some(Prerequisite::JournalpostOpprettet { journalpost_id }),
                "Kan ikke avskrive journalpost: journalpost finnes ikke i state",
            ));
        };

        if state.avskrevet {
            return Ok(StepOutcome::AlreadyCompleted);
        }

        if !state.journalfoert {
            return Ok(StepOutcome::blocked(
                Some(Prerequisite::JournalpostJournalfoert { journalpost_id }),
                "Kan ikke avskrive journalpost: journalpost er ikke journalfort",
            ));
        }

        kan_avskrive_journalpost(&til_journalpost_rule_state(&state))?;

        let journalpostnummer = match state.journalpostnummer {
            Some(journalpostnummer) => journalpostnummer,
            None => {
                return Ok(StepOutcome::blocked(
                    Some(Prerequisite::JournalpostnummerTildelt { journalpost_id }),
                    "Journalpostnummer mangler",
                ))
            }
        };

        self.arkiv_gateway
            .avskriv_journalpost(journalpostnummer, "TE")
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        self.state_repo
            .anvend_journalpost_avskrevet(journalpost_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(StepOutcome::Completed)
    }
}
