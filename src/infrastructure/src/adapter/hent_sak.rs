use anyhow::anyhow;
use application::services::hent_sak::SakRepository;
use async_trait::async_trait;
use domain::model::sak::SakKey;

use crate::mapping;

pub struct SikriRepository;

#[async_trait]
impl SakRepository for SikriRepository {
    async fn hent_sak(
        &self,
        key: SakKey,
        inkluder_journalposter: bool,
    ) -> Result<domain::model::sak::Sak, anyhow::Error> {
        let saksnummer: domain::model::sak::Saksnummer = match key {
            SakKey::SkuffenId(_uuid) => {
                return Err(anyhow!("Har ikke implementert skuffen id enda."))
            }
            SakKey::ArkivId(saksnummer) => saksnummer,
        };
        let sak_reponse =
            sikri_client::hent_sak(saksnummer.as_str(), "SKUFFEN", inkluder_journalposter).await?;
        Ok(mapping::fra_sikri_til_domene::sak::from_sikri_sak_to_domain_sak(sak_reponse)?)
    }
}
