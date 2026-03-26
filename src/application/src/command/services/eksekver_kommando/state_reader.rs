use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::regler::{JournalpostRuleState, SakRuleState};
use domain::eksekvering::typer::EksekveringFeil;

use crate::command::ports::execution_snapshot_port::{DokumentState, JournalpostState, SakState};

use super::EksekverKommandoService;

impl EksekverKommandoService {
    pub(super) async fn hent_sak_state(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<SakState>, EksekveringFeil> {
        self.snapshot_repo
            .hent_sak_state(sak_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))
    }

    pub(super) async fn hent_journalpost_state(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<JournalpostState>, EksekveringFeil> {
        self.snapshot_repo
            .hent_journalpost_state(journalpost_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))
    }

    pub(super) async fn hent_dokument_state(
        &self,
        dokument_id: SkuffenDokumentId,
    ) -> Result<Option<DokumentState>, EksekveringFeil> {
        self.snapshot_repo
            .hent_dokument_state(dokument_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))
    }

    pub(super) async fn hent_journalposter_for_sak(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Vec<JournalpostState>, EksekveringFeil> {
        self.snapshot_repo
            .hent_journalposter_for_sak(sak_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))
    }

    pub(super) async fn hent_saksnummer_for_sak(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<String>, EksekveringFeil> {
        Ok(self
            .hent_sak_state(sak_id)
            .await?
            .and_then(|state| state.saksnummer))
    }
}

pub(super) fn til_sak_rule_state(state: &SakState) -> SakRuleState {
    SakRuleState {
        avsluttet: matches!(
            state.status,
            crate::command::ports::execution_snapshot_port::SakStatus::Avsluttet
        ),
    }
}

pub(super) fn til_journalpost_rule_state(state: &JournalpostState) -> JournalpostRuleState {
    JournalpostRuleState {
        journalpost_type: state.journalposttype,
        journalfoert: state.journalfoert,
        avskrevet: state.avskrevet,
        ekspedert: state.ekspedert,
        har_feilede_dokumenter: state.har_feilede_dokumenter,
        med_utsending: state.med_utsending,
        journalpostnummer: state.journalpostnummer,
    }
}
