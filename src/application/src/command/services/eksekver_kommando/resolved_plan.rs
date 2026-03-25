use domain::eksekvering::plan::{JournalpostType, Utsending};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedDocument {
    pub dokument_id: Uuid,
    pub client_reference: Uuid,
}

#[derive(Debug, Clone)]
pub struct ResolvedJournalpostPlan {
    pub journalpost_id: Uuid,
    pub journalpost_client_reference: Uuid,
    pub sak_id: Uuid,
    pub journalpost_type: JournalpostType,
    pub utsending: Option<Utsending>,
    pub dokumenter: Vec<ResolvedDocument>,
}

#[derive(Debug, Clone)]
pub enum ResolvedStep {
    OpprettSak {
        sak_id: Uuid,
        sak_client_reference: Uuid,
    },
    OpprettJournalpost {
        plan: ResolvedJournalpostPlan,
    },
    LeggTilDokument {
        journalpost_id: Uuid,
        dokument_id: Uuid,
        dokument_client_reference: Uuid,
    },
    Journalfoer {
        journalpost_id: Uuid,
    },
    Avskriv {
        journalpost_id: Uuid,
    },
    AvsluttSak {
        sak_id: Uuid,
    },
}

#[derive(Debug, Clone)]
pub struct ResolvedPlan {
    pub steg: Vec<ResolvedStep>,
}
