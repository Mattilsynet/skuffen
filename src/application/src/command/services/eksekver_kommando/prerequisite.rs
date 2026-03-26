use domain::eksekvering::id::{SkuffenJournalpostId, SkuffenSakId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prerequisite {
    SakOpprettet {
        sak_id: SkuffenSakId,
    },
    SaksnummerTildelt {
        sak_id: SkuffenSakId,
    },
    JournalpostOpprettet {
        journalpost_id: SkuffenJournalpostId,
    },
    JournalpostnummerTildelt {
        journalpost_id: SkuffenJournalpostId,
    },
    JournalpostJournalfoert {
        journalpost_id: SkuffenJournalpostId,
    },
}

impl Prerequisite {
    pub fn as_error_code(&self) -> lib_schemas::skuffen::status::SkuffenStatusErrorCode {
        lib_schemas::skuffen::status::SkuffenStatusErrorCode::PrerequisitePending
    }
}
