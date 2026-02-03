use application::services::hent_sak::SakRepository;
use async_trait::async_trait;

use crate::mapping::{self, lookup::key_mapping_queries::lookup_arkiv_id_fra_skuffen_id};

#[derive(Debug)]
pub struct SikriRepository;

#[async_trait]
impl SakRepository for SikriRepository {
    #[tracing::instrument()]
    async fn hent_sak(
        &self,
        key: domain::model::sak::SakKey,
        inkluder_journalposter: bool,
    ) -> Result<domain::model::sak::Sak, anyhow::Error> {
        let saksnummer: domain::model::sak::Saksnummer =
            lookup_arkiv_id_fra_skuffen_id(key.skuffen_id).await?;
        let sak_reponse =
            sikri_client::hent_sak(saksnummer.as_str(), "SKUFFEN", inkluder_journalposter).await?;
        Ok(mapping::fra_sikri_til_domene::sak::from_sikri_sak_to_domain_sak(sak_reponse).await?)
    }
}
