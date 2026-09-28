use std::fmt::Debug;

use async_trait::async_trait;
use domain::model::sak::{Sak, SakKey, Saksnummer};
use uuid::Uuid;

use crate::query::ports::use_cases::HentSakUseCase;

#[async_trait]
pub trait SakRepository {
    async fn hent_sak(
        &self,
        saksnummer: Saksnummer,
        inkluder_journalposter: bool,
    ) -> Result<Sak, anyhow::Error>;
}

/// Lokalt oppslag fra klientreferanse til saksnummer. Leser bare; oppretter
/// aldri identitet.
#[async_trait]
pub trait SaksnummerOppslag {
    async fn saksnummer_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<Saksnummer, anyhow::Error>;
}

pub struct HentSakService {
    repo: Box<dyn SakRepository + Send + Sync>,
    oppslag: Box<dyn SaksnummerOppslag + Send + Sync>,
}

impl Debug for HentSakService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HentSakService").finish_non_exhaustive()
    }
}

impl HentSakService {
    pub fn new(
        repo: Box<dyn SakRepository + Send + Sync>,
        oppslag: Box<dyn SaksnummerOppslag + Send + Sync>,
    ) -> Self {
        Self { repo, oppslag }
    }
}

#[async_trait]
impl HentSakUseCase for HentSakService {
    async fn handle(
        &self,
        req: SakKey,
        inkluder_journalposter: bool,
    ) -> Result<Sak, anyhow::Error> {
        let saksnummer = match req {
            SakKey::ArkivId(saksnummer) => saksnummer,
            SakKey::ClientReference(client_reference) => {
                self.oppslag
                    .saksnummer_for_client_reference(client_reference)
                    .await?
            }
        };
        self.repo.hent_sak(saksnummer, inkluder_journalposter).await
    }
}
