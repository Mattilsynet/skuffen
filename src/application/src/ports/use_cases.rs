use anyhow;
use async_trait::async_trait;
use lib_schemas::arkiv::v2::{journalpost::HentJournalpostRequest, sak::HentSakRequest};

#[async_trait]
pub trait QueryUseCase {
    type Request;
    type Response;

    async fn handle(&self, req: Self::Request) -> Result<Self::Response, anyhow::Error>;
}

#[async_trait]
pub trait HentSakUseCase: Send + Sync {
    async fn handle(
        &self,
        req: HentSakRequest,
    ) -> Result<lib_schemas::arkiv::v2::sak::SakResponse, anyhow::Error>;
}

#[async_trait]
pub trait HentJournalpostUseCase: Send + Sync {
    async fn handle(
        &self,
        req: HentJournalpostRequest,
    ) -> Result<lib_schemas::arkiv::v2::journalpost::JournalpostResponse, anyhow::Error>;
}
