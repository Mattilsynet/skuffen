//! Materialiserte attributter (SKU-0016 R12).
//!
//! Dekomponering skriver disse inn i state-tabellene. Executor leser dem
//! derfra og rører aldri `kommando.payload`. Det fjerner den posisjonelle
//! koblingen mellom id-liste og payload-liste som fantes i v2.

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

/// Korrespondanseparter, én variant per journalpostform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Korrespondanseparter {
    /// Inngående: nøyaktig én avsender.
    Avsender(Korrespondansepart),
    /// Utgående uten utsending.
    Mottakere(Vec<Korrespondansepart>),
    /// Utgående med utsending — krever full digital adresse.
    Utsendingsmottakere(Vec<Utsendingsmottaker>),
    /// Internt notat har ingen.
    Ingen,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalpostAttributter {
    /// Klientens referanse. Brukes som stabil identifikator i
    /// arkivmapping-diagnostikk.
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

/// Hvor dokumentinnholdet kommer fra, med referansene arkivkallet trenger.
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
    /// Referansen arkivet skal få. For en mal er det den rendrede PDF-en;
    /// original `mal_referanse` sendes aldri (SKU-0005).
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

/// Alt dekomponering skal skrive, som én enhet. Skrives i én transaksjon
/// (SKU-0016 R12) slik at en crash aldri etterlater orphan-rader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dekomponeringsplan {
    pub command_id: Uuid,
    pub sak: SakRad,
    pub journalpost: Option<JournalpostRad>,
    pub dokumenter: Vec<DokumentRad>,
    pub operasjoner: Vec<OperasjonRad>,
}

/// Sakens rad. `attributter` er `None` når kommandoen ikke oppretter saken,
/// og raden da bare skal sikres å eksistere.
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
