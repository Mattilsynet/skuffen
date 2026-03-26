use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::plan::Utsending;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedDocument {
    pub dokument_id: SkuffenDokumentId,
    pub client_reference: Uuid,
}

#[derive(Debug, Clone)]
pub struct ResolvedJournalpostPlan {
    pub journalpost_id: SkuffenJournalpostId,
    pub journalpost_client_reference: Uuid,
    pub sak_id: SkuffenSakId,
    pub utsending: Option<Utsending>,
    pub dokumenter: Vec<ResolvedDocument>,
}

#[derive(Debug, Clone)]
pub enum ResolvedStep {
    OpprettSak {
        sak_id: SkuffenSakId,
        sak_client_reference: Uuid,
    },
    OpprettJournalpost {
        plan: ResolvedJournalpostPlan,
    },
    LeggTilDokument {
        journalpost_id: SkuffenJournalpostId,
        dokument_id: SkuffenDokumentId,
        dokument_client_reference: Uuid,
    },
    Journalfoer {
        journalpost_id: SkuffenJournalpostId,
    },
    Avskriv {
        journalpost_id: SkuffenJournalpostId,
    },
    AvsluttSak {
        sak_id: SkuffenSakId,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedPlan {
    pub steg: Vec<ResolvedStep>,
}
