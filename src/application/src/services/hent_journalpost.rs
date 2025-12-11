// application/src/services.rs
use async_trait::async_trait;
use domain::model::journalpost::{Journalpost, JournalpostKey};

use crate::ports::use_cases::HentJournalpostUseCase;

#[async_trait]
pub trait JournalpostRepository {
    async fn hent_journalpost(&self, id: JournalpostKey) -> Result<Journalpost, anyhow::Error>;
}

#[derive(Debug)]
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
    async fn handle(&self, req: JournalpostKey) -> Result<Journalpost, anyhow::Error> {
        self.repo.hent_journalpost(req).await
    }
}
