use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId};
use domain::eksekvering::typer::EksekveringFeil;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use uuid::Uuid;

use super::prerequisite::Prerequisite;
use super::step_outcome::StepOutcome;
use super::EksekverKommandoService;

impl EksekverKommandoService {
    pub(super) async fn legg_til_dokument(
        &self,
        envelope: &CommandEnvelope<Command>,
        journalpost_id: SkuffenJournalpostId,
        dokument_id: SkuffenDokumentId,
        dokument_client_reference: Uuid,
        report: &mut super::execution_report::ExecutionReport,
    ) -> Result<StepOutcome, EksekveringFeil> {
        let Some(journalpost_state) = self.hent_journalpost_state(journalpost_id).await? else {
            return Ok(StepOutcome::blocked(
                Some(Prerequisite::JournalpostOpprettet { journalpost_id }),
                "Kan ikke legge til dokument: journalpost finnes ikke i state ennå",
            ));
        };

        if self
            .hent_dokument_state(dokument_id)
            .await?
            .is_some_and(|existing| existing.lagt_til)
        {
            let outcome = StepOutcome::AlreadyCompleted;
            self.maybe_wake_after_dokument_step(envelope, journalpost_id, &outcome)
                .await?;
            return Ok(outcome);
        }

        let journalpostnummer = match journalpost_state.journalpostnummer {
            Some(journalpostnummer) => journalpostnummer,
            None => {
                return Ok(StepOutcome::blocked(
                    Some(Prerequisite::JournalpostnummerTildelt { journalpost_id }),
                    "Journalpostnummer mangler",
                ))
            }
        };

        let resp = match self
            .arkiv_gateway
            .legg_til_vedlegg(envelope, journalpostnummer, vec![dokument_client_reference])
            .await
        {
            Ok(resp) => resp,
            Err(err) => {
                let err = self.map_arkiv_feil(err);
                if matches!(
                    err.feiltype,
                    domain::eksekvering::typer::EksekveringFeiltype::Irrecoverable
                ) {
                    self.snapshot_repo
                        .anvend_dokument_irrecoverable_feil(dokument_id, journalpost_id)
                        .await
                        .map_err(|persist_err| {
                            EksekveringFeil::recoverable(format!(
                                "Klarte ikke a persistere irrecoverable dokumentfeil: {}",
                                persist_err
                            ))
                        })?;

                    self.wake_journalpost(journalpost_id)
                        .await
                        .map_err(|wake_err| {
                            EksekveringFeil::recoverable(format!(
                                "Klarte ikke wake ventende kommandoer etter dokumentfeil: {}",
                                wake_err.melding
                            ))
                        })?;
                    self.wake_sak_for_journalpost_envelope(envelope)
                        .await
                        .map_err(|wake_err| {
                            EksekveringFeil::recoverable(format!(
                                "Klarte ikke wake sak-scope etter dokumentfeil: {}",
                                wake_err.melding
                            ))
                        })?;
                }
                return Err(err);
            }
        };

        if let Some(Some(arkiv_id)) = resp.into_iter().next() {
            self.id_mapping
                .oppdater_arkiv_id_for_client_reference(
                    dokument_client_reference,
                    arkiv_id.to_string(),
                )
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
            report.add_dokument_id(arkiv_id.to_string());
        }

        self.snapshot_repo
            .anvend_dokument_lagt_til(dokument_id, journalpost_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        let outcome = StepOutcome::Completed;
        self.maybe_wake_after_dokument_step(envelope, journalpost_id, &outcome)
            .await?;
        Ok(outcome)
    }
}
