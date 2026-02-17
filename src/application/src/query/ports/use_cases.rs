use anyhow;
use async_trait::async_trait;
use domain::model::{
    journalpost::{Journalpost, JournalpostKey},
    sak::{Sak, SakKey},
};

#[async_trait]
pub trait QueryUseCase {
    type Request;
    type Response;

    async fn handle(&self, req: Self::Request) -> Result<Self::Response, anyhow::Error>;
}

#[async_trait]
pub trait HentSakUseCase: Send + Sync {
    async fn handle(&self, req: SakKey, inkluder_journalposter: bool)
        -> Result<Sak, anyhow::Error>;
}

#[async_trait]
pub trait HentJournalpostUseCase: Send + Sync {
    async fn handle(&self, req: JournalpostKey) -> Result<Journalpost, anyhow::Error>;
}
