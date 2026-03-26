use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};

use crate::command::ports::execution_snapshot_port::{DokumentState, JournalpostState, SakState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SakStateRegistration {
    pub sak_id: SkuffenSakId,
    pub state: SakState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalpostStateRegistration {
    pub journalpost_id: SkuffenJournalpostId,
    pub sak_id: SkuffenSakId,
    pub state: JournalpostState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DokumentStateRegistration {
    pub dokument_id: SkuffenDokumentId,
    pub journalpost_id: SkuffenJournalpostId,
    pub state: DokumentState,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EksekveringssystemRegistration {
    pub sak: Option<SakStateRegistration>,
    pub journalpost: Option<JournalpostStateRegistration>,
    pub dokumenter: Vec<DokumentStateRegistration>,
}
