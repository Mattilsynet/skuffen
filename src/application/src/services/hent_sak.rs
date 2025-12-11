use std::fmt::Debug;

use async_trait::async_trait;
use domain::model::sak::{Sak, SakKey};

use crate::ports::use_cases::HentSakUseCase;

#[async_trait]
pub trait SakRepository {
    async fn hent_sak(
        &self,
        id: SakKey,
        inkluder_journalposter: bool,
    ) -> Result<Sak, anyhow::Error>;
}

#[derive(Debug)]
pub struct HentSakService<R>
where
    R: Debug,
{
    repo: R,
}

impl<R> HentSakService<R>
where
    R: Debug,
{
    pub fn new(repo: R) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl<R> HentSakUseCase for HentSakService<R>
where
    R: SakRepository + Send + Sync + Debug,
{
    async fn handle(
        &self,
        req: SakKey,
        inkluder_journalposter: bool,
    ) -> Result<Sak, anyhow::Error> {
        self.repo.hent_sak(req, inkluder_journalposter).await
    }
}
