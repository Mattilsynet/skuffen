use domain::eksekvering::execution::Ventegrunn;
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

    pub fn as_ventegrunn(&self) -> Ventegrunn {
        match self {
            Self::SakOpprettet { sak_id } => Ventegrunn::SakOpprettet { sak_id: *sak_id },
            Self::SaksnummerTildelt { sak_id } => Ventegrunn::SaksnummerTildelt { sak_id: *sak_id },
            Self::JournalpostOpprettet { journalpost_id } => Ventegrunn::JournalpostOpprettet {
                journalpost_id: *journalpost_id,
            },
            Self::JournalpostnummerTildelt { journalpost_id } => {
                Ventegrunn::JournalpostnummerTildelt {
                    journalpost_id: *journalpost_id,
                }
            }
            Self::JournalpostJournalfoert { journalpost_id } => {
                Ventegrunn::JournalpostJournalfoert {
                    journalpost_id: *journalpost_id,
                }
            }
        }
    }
}
