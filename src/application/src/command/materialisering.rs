//! Materialiserte attributter (SKU-0016 R12).
//!
//! Dekomponering skriver disse inn i state-tabellene, og executor leser dem
//! derfra.

use domain::eksekvering::html_template::TemplateFelt;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::JournalpostType;
use uuid::Uuid;

use crate::command::model::{Arkivdel, Korrespondansepart, Utsendingsmottaker};

/// Skjerming, flatet ut slik state-tabellene lagrer den.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tilgang {
    pub tilgangskode: Option<String>,
    pub tilgangshjemmel: Option<String>,
}

impl Tilgang {
    pub fn er_skjermet(&self) -> bool {
        self.tilgangskode.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SakAttributter {
    pub sakstittel: String,
    pub arkivdel: Arkivdel,
    pub ordningsverdi: String,
    pub saksbehandler_id: String,
    pub saksbehandler_enhet: String,
    pub tilgang: Tilgang,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Korrespondanseparter {
    /// Inngående: nøyaktig én.
    Avsender(Korrespondansepart),
    Mottakere(Vec<Korrespondansepart>),
    /// Krever full digital adresse.
    Utsendingsmottakere(Vec<Utsendingsmottaker>),
    Ingen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalpostAttributter {
    /// Stabil identifikator i arkivmapping-diagnostikk.
    pub client_reference: Uuid,
    pub tittel: String,
    pub dokument_dato: String,
    pub journalposttype: JournalpostType,
    pub med_utsending: bool,
    pub saksbehandler_id: String,
    pub saksbehandler_enhet: String,
    pub tilgang: Tilgang,
    pub korrespondanseparter: Korrespondanseparter,
    pub kildesystem: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dokumentkilde {
    Bytes {
        dokument_referanse: Uuid,
        filtype: String,
    },
    HtmlTemplate {
        mal_referanse: Uuid,
        felter: Vec<TemplateFelt>,
        rendered_dokument_referanse: Option<Uuid>,
    },
}

impl Dokumentkilde {
    /// For en mal er det den rendrede PDF-en; `mal_referanse` sendes aldri
    /// til arkivet (SKU-0005).
    pub fn arkivreferanse(&self) -> Option<Uuid> {
        match self {
            Self::Bytes {
                dokument_referanse, ..
            } => Some(*dokument_referanse),
            Self::HtmlTemplate {
                rendered_dokument_referanse,
                ..
            } => *rendered_dokument_referanse,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DokumentAttributter {
    pub tittel: String,
    pub rekkefolge: u16,
    pub kilde: Dokumentkilde,
}

impl DokumentAttributter {
    pub fn er_hoveddokument(&self) -> bool {
        self.rekkefolge == 0
    }
}

// ---------------------------------------------------------------------------
// Dekomponeringsplanen
// ---------------------------------------------------------------------------

/// Skrives i én transaksjon (SKU-0016 R12), så en crash ikke kan etterlate
/// orphan-rader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dekomponeringsplan {
    pub command_id: Uuid,
    pub sak: SakRad,
    pub journalpost: Option<JournalpostRad>,
    pub dokumenter: Vec<DokumentRad>,
    pub operasjoner: Vec<OperasjonRad>,
}

/// `attributter` er `None` når kommandoen ikke oppretter saken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SakRad {
    pub sak_id: SkuffenSakId,
    pub client_reference: Option<Uuid>,
    pub arkiv_id: Option<String>,
    pub attributter: Option<SakAttributter>,
    pub oensket_saksansvarlig: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalpostRad {
    pub journalpost_id: SkuffenJournalpostId,
    pub client_reference: Uuid,
    pub attributter: JournalpostAttributter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DokumentRad {
    pub dokument_id: SkuffenDokumentId,
    pub journalpost_id: SkuffenJournalpostId,
    pub client_reference: Uuid,
    pub attributter: DokumentAttributter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperasjonRad {
    pub operasjon_id: domain::eksekvering::operasjon::OperasjonId,
    pub operasjonstype: domain::eksekvering::operasjon::Operasjonstype,
    pub entitet_id: domain::eksekvering::operasjon::EntitetId,
    pub sak_id: SkuffenSakId,
}
