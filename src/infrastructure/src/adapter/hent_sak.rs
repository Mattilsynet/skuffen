use anyhow::anyhow;
use application::services::hent_sak::SakRepository;
use async_trait::async_trait;
use lib_schemas::arkiv::v2::sak::{SakKey, SakResponse, Saksnummer};

pub struct SikriRepository;

#[async_trait]
impl SakRepository for SikriRepository {
    async fn hent_sak(&self, key: SakKey) -> Result<SakResponse, anyhow::Error> {
        let saksnummer: Saksnummer = match key {
            SakKey::SkuffenId(_uuid) => {
                return Err(anyhow!("Har ikke implementert skuffen id enda."))
            }
            SakKey::ArkivId(saksnummer) => saksnummer,
        };
        let sak_reponse = sikri_client::hent_sak(saksnummer.as_str(), "SKUFFEN").await?;
        Ok(sak_reponse.into())
    }
}
