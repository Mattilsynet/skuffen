use async_trait::async_trait;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::SakMedBarn;

use crate::command::materialisering::{
    DokumentAttributter, JournalpostAttributter, SakAttributter,
};

/// Hva som er sant nå. Sletter du alle operasjonsrader, skal dette laget
/// fortsatt kunne svare på «hva er sant om denne saken?» (SKU-0016).
#[async_trait]
pub trait FaktaRepository: Send + Sync {
    /// Faktabildet domenefunksjonene vurderer mot.
    async fn hent_sak_med_barn(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<SakMedBarn>, anyhow::Error>;

    /// Materialiserte attributter, så executor slipper å lese payload.
    async fn hent_sak_attributter(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<SakAttributter>, anyhow::Error>;

    async fn hent_journalpost_attributter(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<JournalpostAttributter>, anyhow::Error>;

    async fn hent_dokument_attributter(
        &self,
        dokument_id: SkuffenDokumentId,
    ) -> Result<Option<DokumentAttributter>, anyhow::Error>;

    /// Alle dokumentene på en journalpost, sortert på `rekkefolge`.
    async fn hent_dokumenter_for_journalpost(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Vec<(SkuffenDokumentId, DokumentAttributter)>, anyhow::Error>;
}
