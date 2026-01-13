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

// #[derive(Debug)] // removed derive
pub struct HentSakService {
    repo: Box<dyn SakRepository + Send + Sync>,
}

impl Debug for HentSakService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HentSakService").finish_non_exhaustive()
    }
}

impl HentSakService {
    pub fn new(repo: Box<dyn SakRepository + Send + Sync>) -> Self {
        Self { repo }
    }
}

#[async_trait]
impl HentSakUseCase for HentSakService {
    async fn handle(
        &self,
        req: SakKey,
        inkluder_journalposter: bool,
    ) -> Result<Sak, anyhow::Error> {
        self.repo.hent_sak(req, inkluder_journalposter).await
    }
}
