mod api;
pub mod domain;
mod dto;
mod secret;

use crate::domain::ny_sak::NySak;
use crate::domain::sak::SakRespons;
use crate::dto::elements_sak::ElementsSak;

pub async fn alive() -> anyhow::Result<()> {
    api::alive().await
}

// FIXME Trenger man kildesystem på en GET når det ikke benyttes til filtrering?
pub async fn hent_sak(
    saksnummer: &str,
    kildesystem: &str,
    inkluder_journalposter: bool,
) -> anyhow::Result<SakRespons> {
    let resp = api::get_sak(saksnummer, kildesystem, inkluder_journalposter).await?;
    Ok(SakRespons::from(resp))
}

pub async fn opprett_sak(data: NySak) -> anyhow::Result<SakRespons> {
    let sak = api::create_sak(ElementsSak::from(data)).await?;
    Ok(SakRespons::from(sak))
}
