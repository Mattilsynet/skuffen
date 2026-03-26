use chrono::{DateTime, Utc};

use crate::eksekvering::id::{SkuffenJournalpostId, SkuffenSakId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EksekveringStatus {
    Klar,
    Kjorer,
    Venter,
    RetryVenter,
    Ok,
    Feil,
}

impl EksekveringStatus {
    pub fn as_db_code(self) -> &'static str {
        match self {
            Self::Klar => "klar",
            Self::Kjorer => "kjorer",
            Self::Venter => "venter",
            Self::RetryVenter => "retry_venter",
            Self::Ok => "ok",
            Self::Feil => "feil",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ventegrunn {
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
    SakHarUferdigeJournalposter {
        sak_id: SkuffenSakId,
    },
}

impl Ventegrunn {
    pub fn kind_code(&self) -> &'static str {
        match self {
            Self::SakOpprettet { .. } => "sak_opprettet",
            Self::SaksnummerTildelt { .. } => "saksnummer_tildelt",
            Self::JournalpostOpprettet { .. } => "journalpost_opprettet",
            Self::JournalpostnummerTildelt { .. } => "journalpostnummer_tildelt",
            Self::JournalpostJournalfoert { .. } => "journalpost_journalfoert",
            Self::SakHarUferdigeJournalposter { .. } => "sak_har_uferdige_journalposter",
        }
    }

    pub fn sak_id(&self) -> Option<SkuffenSakId> {
        match self {
            Self::SakOpprettet { sak_id }
            | Self::SaksnummerTildelt { sak_id }
            | Self::SakHarUferdigeJournalposter { sak_id } => Some(*sak_id),
            _ => None,
        }
    }

    pub fn journalpost_id(&self) -> Option<SkuffenJournalpostId> {
        match self {
            Self::JournalpostOpprettet { journalpost_id }
            | Self::JournalpostnummerTildelt { journalpost_id }
            | Self::JournalpostJournalfoert { journalpost_id } => Some(*journalpost_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kjorbarhet {
    Klar,
    Venter { grunn: Ventegrunn, detalj: String },
    Feil { detalj: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Eksekveringsresultat {
    Ok,
    Venter {
        grunn: Ventegrunn,
        detalj: String,
    },
    RetryVenter {
        detalj: String,
        retry_ready_at: DateTime<Utc>,
    },
    Feil {
        detalj: String,
    },
}
