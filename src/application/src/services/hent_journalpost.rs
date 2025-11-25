// application/src/services.rs
use async_trait::async_trait;
use lib_schemas::arkiv::v2::journalpost::{
    HentJournalpostRequest, JournalpostKey, JournalpostResponse,
};

use crate::ports::use_cases::HentJournalpostUseCase;

#[async_trait]
pub trait JournalpostRepository {
    async fn hent_journalpost(
        &self,
        id: JournalpostKey,
    ) -> Result<JournalpostResponse, anyhow::Error>;
}

pub struct HentJournalpostService<R> {
    repo: R,
}

impl<R> HentJournalpostService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl<R> HentJournalpostUseCase for HentJournalpostService<R>
where
    R: JournalpostRepository + Send + Sync,
{
    async fn handle(
        &self,
        req: HentJournalpostRequest,
    ) -> Result<JournalpostResponse, anyhow::Error> {
        self.repo.hent_journalpost(req.key).await
    }
}
