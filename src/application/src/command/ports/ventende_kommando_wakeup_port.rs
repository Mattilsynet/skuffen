use async_trait::async_trait;
use domain::eksekvering::id::{SkuffenJournalpostId, SkuffenSakId};

#[async_trait]
pub trait VentendeKommandoWakeup: Send + Sync {
    async fn etter_sak_endret(&self, sak_id: SkuffenSakId) -> Result<(), anyhow::Error>;
    async fn etter_journalpost_endret(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<(), anyhow::Error>;
}
