use anyhow::Result;
use async_trait::async_trait;
use domain::model::sak::{Sak, SakKey};

#[async_trait]
pub trait SakPort {
    async fn hent(&self, sak_key: SakKey) -> Result<Sak>;
    async fn opprett(); //TODO:
    async fn avslutt(); //TODO:
}
