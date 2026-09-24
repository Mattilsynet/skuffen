//! Dekomponeringsinput: det minimum av command payload som avgjør
//! operasjonslisten (SKU-0016 R2).
//!
//! Domain importerer aldri wire-typer (SKU-0013).

use crate::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use crate::eksekvering::tilstand::JournalpostType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dokumentkilde {
    Bytes,
    /// Må rendres til PDF før bruk.
    HtmlTemplate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DokumentSpesifikasjon {
    pub dokument_id: SkuffenDokumentId,
    /// Posisjon i kommandoens dokumentliste. `0` er hoveddokumentet.
    pub rekkefolge: u16,
    pub kilde: Dokumentkilde,
}

/// Utsnittet av kommandoen som avgjør operasjonslisten.
/// `application::command::Command` bærer hele payloaden.
///
/// De fire journalpost-kommandoene kollapser til én variant: `journalposttype`
/// og `med_utsending` er det eneste som skiller operasjonslistene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dekomponeringsinput {
    OpprettSak {
        sak_id: SkuffenSakId,
    },
    OpprettJournalpost {
        sak_id: SkuffenSakId,
        journalpost_id: SkuffenJournalpostId,
        journalposttype: JournalpostType,
        med_utsending: bool,
        dokumenter: Vec<DokumentSpesifikasjon>,
    },
    AvsluttSak {
        sak_id: SkuffenSakId,
    },
    SettSaksansvarlig {
        sak_id: SkuffenSakId,
    },
}

impl Dekomponeringsinput {
    pub fn sak_id(&self) -> SkuffenSakId {
        match self {
            Self::OpprettSak { sak_id }
            | Self::AvsluttSak { sak_id }
            | Self::SettSaksansvarlig { sak_id }
            | Self::OpprettJournalpost { sak_id, .. } => *sak_id,
        }
    }

    pub fn journalpost_id(&self) -> Option<SkuffenJournalpostId> {
        match self {
            Self::OpprettJournalpost { journalpost_id, .. } => Some(*journalpost_id),
            _ => None,
        }
    }
}
