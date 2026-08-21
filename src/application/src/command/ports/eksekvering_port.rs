use async_trait::async_trait;
use uuid::Uuid;

use crate::command::materialisering::{
    DokumentAttributter, JournalpostAttributter, SakAttributter,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservertJournalstatus {
    /// `R`
    Reservert,
    /// `F`
    KlarForEkspedering,
    /// `E`
    Ekspedert,
    /// `J`
    Journalfoert,
    /// Behandles som «ikke ferdig ennå».
    Annet,
}

/// Statuskodene Skuffen selv setter. På utgående settes `J` av RPA
/// (SKU-0016 R10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Journalstatus {
    /// `J`. Kun inngående og internt notat.
    Journalfoert,
    /// `E`. Utgående uten utsending.
    Ekspedert,
    /// `F`. Trigger SvarUt.
    KlarForEkspedering,
}

impl Journalstatus {
    pub fn as_arkivkode(self) -> &'static str {
        match self {
            Self::Journalfoert => "J",
            Self::Ekspedert => "E",
            Self::KlarForEkspedering => "F",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpprettSakResultat {
    pub saksnummer: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpprettJournalpostResultat {
    pub journalpost_id: i32,
}

/// Alle argumenter er materialiserte attributter, så executor aldri rører
/// wire-typer (SKU-0016 R12).
#[async_trait]
pub trait ArkivGateway: Send + Sync {
    async fn opprett_sak(
        &self,
        attributter: &SakAttributter,
    ) -> Result<OpprettSakResultat, anyhow::Error>;

    async fn opprett_journalpost(
        &self,
        saksnummer: &str,
        journalpost: &JournalpostAttributter,
        hoveddokument: &DokumentAttributter,
    ) -> Result<OpprettJournalpostResultat, anyhow::Error>;

    /// Ett om gangen (D5). Sikris batch-API returnerer `Vec<Option<i32>>`,
    /// der partial success ikke er håndterbart.
    async fn legg_til_vedlegg(
        &self,
        journalpost_id: i32,
        vedlegg: &DokumentAttributter,
    ) -> Result<Option<i32>, anyhow::Error>;

    async fn sett_journalpost_status(
        &self,
        journalpost_id: i32,
        status: Journalstatus,
    ) -> Result<(), anyhow::Error>;

    /// Kun inngående avskrives (D21). `TE` — tatt til etterretning.
    async fn avskriv_journalpost(&self, journalpost_id: i32) -> Result<(), anyhow::Error>;

    /// Ren observasjon.
    async fn hent_journalstatus(
        &self,
        journalpost_id: i32,
    ) -> Result<ObservertJournalstatus, anyhow::Error>;

    async fn avslutt_sak(&self, saksnummer: &str) -> Result<(), anyhow::Error>;

    async fn sett_saksansvarlig(
        &self,
        saksnummer: &str,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
    ) -> Result<(), anyhow::Error>;
}

/// Lagret på deterministisk nøkkel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderResultat {
    pub rendered_dokument_referanse: Uuid,
}
