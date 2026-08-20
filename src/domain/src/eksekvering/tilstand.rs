//! Fakta om entitetene i en sak: hva som er sant nå.
//!
//! Fakta er skilt fra eksekvering. Sletter du alle operasjonsrader, skal disse
//! typene fortsatt kunne svare på «hva er sant om denne saken?» (SKU-0016).

use crate::eksekvering::html_template::TemplateFelt;
use crate::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalpostType {
    Inngaende,
    Utgaaende,
    InterntNotat,
}

impl JournalpostType {
    /// Arkivkoden Sikri bruker for journalposttypen.
    pub fn as_arkivkode(self) -> &'static str {
        match self {
            Self::Inngaende => "I",
            Self::Utgaaende => "U",
            Self::InterntNotat => "X",
        }
    }
}

/// Saksansvarlig (Noark 5 M306) — identifiserer ansvarlig saksbehandler og enhet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saksansvarlig {
    pub saksbehandler_id: String,
    pub enhet: String,
}

// ---------------------------------------------------------------------------
// Tilstander
// ---------------------------------------------------------------------------

/// Sakens tilstand i arkivet.
///
/// Ingen feiltilstand her med vilje: at et forsøk på å opprette saken feilet
/// er eksekvering, ikke et faktum om saken. Det bor på `operasjon.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SakTilstand {
    IkkeOpprettet,
    Opprettet,
    Avsluttet,
}

/// Journalpostens observerte arkivstatus.
///
/// Skuffen oppretter aldri en journalpost direkte i `Journalfoert`; hver
/// statusovergang er en egen operasjon (SKU-0016 R10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalpostTilstand {
    /// Finnes ikke i arkivet ennå.
    IkkeOpprettet,
    /// Opprettet. `I`/`X` i sin åpne startstatus, `U` i `R`.
    Opprettet,
    /// `F` — klar for ekspedering via SvarUt.
    KlarForEkspedering,
    /// `E` — ekspedert.
    Ekspedert,
    /// `J` — journalført og låst.
    Journalfoert,
    /// Avskrevet (`TE`). Kun inngående.
    Avskrevet,
}

/// Dokumentets livsløp fra innhold til arkiv.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DokumentTilstand {
    /// HTML-mal som ennå ikke er rendret til PDF.
    AvventerRendring,
    /// Innholdet finnes og er klart, men dokumentet er ikke i arkivet ennå.
    Klar,
    /// Dokumentet ligger i arkivet.
    Ok,
}

// ---------------------------------------------------------------------------
// Aggregat-snapshots
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SakMedBarn {
    pub sak_id: SkuffenSakId,
    pub tilstand: SakTilstand,
    /// Sakens arkiv-id (saksnummer). Eneste kilde er `entitet.arkiv_id`.
    pub arkiv_id: Option<String>,
    /// Ønsket saksansvarlig (Noark 5 M306), materialisert ved dekomponering.
    pub oensket_saksansvarlig: Option<Saksansvarlig>,
    /// Nåværende saksansvarlig satt i arkivet.
    pub naavaerende_saksansvarlig: Option<Saksansvarlig>,
    pub journalposter: Vec<JournalpostMedDokumenter>,
}

#[derive(Debug, Clone)]
pub struct JournalpostMedDokumenter {
    pub journalpost_id: SkuffenJournalpostId,
    pub tilstand: JournalpostTilstand,
    /// Journalpostens arkiv-id. Eneste kilde er `entitet.arkiv_id`.
    pub arkiv_id: Option<String>,
    pub journalposttype: JournalpostType,
    pub med_utsending: bool,
    pub dokumenter: Vec<DokumentMedTilstand>,
}

#[derive(Debug, Clone)]
pub struct DokumentMedTilstand {
    pub dokument_id: SkuffenDokumentId,
    pub tilstand: DokumentTilstand,
    /// Posisjon i kommandoens dokumentliste. `0` er hoveddokumentet.
    pub rekkefolge: u16,
    pub kilde: DokumentKildeTilstand,
}

impl DokumentMedTilstand {
    pub fn er_hoveddokument(&self) -> bool {
        self.rekkefolge == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DokumentKildeTilstand {
    Bytes,
    HtmlTemplate {
        mal_referanse: uuid::Uuid,
        felter: Vec<TemplateFelt>,
        rendered_dokument_referanse: Option<uuid::Uuid>,
    },
}

// ---------------------------------------------------------------------------
// Oppslag
// ---------------------------------------------------------------------------

impl SakMedBarn {
    pub fn journalpost(&self, id: SkuffenJournalpostId) -> Option<&JournalpostMedDokumenter> {
        self.journalposter.iter().find(|jp| jp.journalpost_id == id)
    }

    /// Finner dokumentet og journalposten det hører til.
    pub fn dokument(
        &self,
        id: SkuffenDokumentId,
    ) -> Option<(&JournalpostMedDokumenter, &DokumentMedTilstand)> {
        self.journalposter.iter().find_map(|jp| {
            jp.dokumenter
                .iter()
                .find(|dok| dok.dokument_id == id)
                .map(|dok| (jp, dok))
        })
    }
}

impl JournalpostMedDokumenter {
    pub fn hoveddokument(&self) -> Option<&DokumentMedTilstand> {
        self.dokumenter.iter().find(|dok| dok.er_hoveddokument())
    }
}
