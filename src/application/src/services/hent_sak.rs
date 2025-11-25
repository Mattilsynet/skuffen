use async_trait::async_trait;
use lib_schemas::arkiv::v2::sak::{HentSakRequest, SakKey, SakResponse};

use crate::ports::use_cases::HentSakUseCase;

#[async_trait]
pub trait SakRepository {
    async fn hent_sak(&self, id: SakKey) -> Result<SakResponse, anyhow::Error>;
}
pub struct HentSakService<R> {
    repo: R,
}

impl<R> HentSakService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl<R> HentSakUseCase for HentSakService<R>
where
    R: SakRepository + Send + Sync,
{
    async fn handle(&self, req: HentSakRequest) -> Result<SakResponse, anyhow::Error> {
        self.repo.hent_sak(req.key).await
    }
}
