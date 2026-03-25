use domain::eksekvering::typer::EksekveringFeil;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use uuid::Uuid;

use crate::command::ports::eksekvering_state_port::DokumentState;

use super::prerequisite::Prerequisite;
use super::step_outcome::StepOutcome;
use super::EksekverKommandoService;

impl EksekverKommandoService {
    pub(super) async fn legg_til_dokument(
        &self,
        envelope: &CommandEnvelope<Command>,
        journalpost_id: Uuid,
        dokument_id: Uuid,
        dokument_client_reference: Uuid,
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
            return Ok(StepOutcome::AlreadyCompleted);
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

        let resp = self
            .arkiv_gateway
            .legg_til_vedlegg(envelope, journalpostnummer, vec![dokument_client_reference])
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        if let Some(Some(arkiv_id)) = resp.into_iter().next() {
            self.id_mapping
                .oppdater_arkiv_id_for_client_reference(
                    dokument_client_reference,
                    arkiv_id.to_string(),
                )
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        }

        self.state_repo
            .lagre_dokument_state(
                dokument_id,
                journalpost_id,
                DokumentState {
                    lagt_til: true,
                    irrecoverable_feil: false,
                },
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(StepOutcome::Completed)
    }
}
