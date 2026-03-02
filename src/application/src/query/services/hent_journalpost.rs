// application/src/services.rs
use async_trait::async_trait;
use domain::model::journalpost::{Journalpost, JournalpostKey};

use crate::query::ports::use_cases::HentJournalpostUseCase;

#[async_trait]
pub trait JournalpostRepository {
    async fn hent_journalpost(&self, id: JournalpostKey) -> Result<Journalpost, anyhow::Error>;
}

pub struct HentJournalpostService {
    repo: Box<dyn JournalpostRepository + Send + Sync>,
}

impl HentJournalpostService {
    pub fn new(repo: Box<dyn JournalpostRepository + Send + Sync>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl HentJournalpostUseCase for HentJournalpostService {
    async fn handle(&self, req: JournalpostKey) -> Result<Journalpost, anyhow::Error> {
        self.repo.hent_journalpost(req).await
    }
}
