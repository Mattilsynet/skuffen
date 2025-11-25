mod api;
pub mod domain;
mod dto;
mod secret;

use crate::domain::full_sak::FullSak;
use crate::domain::ny_sak::NySak;
use crate::domain::sak::SakResponse;
use crate::dto::elements_sak::ElementsSak;

pub async fn alive() -> anyhow::Result<()> {
    api::alive().await
}

// FIXME Trenger man kildesystem på en GET når det ikke benyttes til filtrering?
pub async fn hent_sak(saksnummer: &str, kildesystem: &str) -> anyhow::Result<SakResponse> {
    let resp = api::get_sak(saksnummer, kildesystem, false).await?;
    Ok(SakResponse::from(resp))
}

pub async fn hent_sak_med_journalposter(
    saksnummer: &str,
    kildesystem: &str,
) -> anyhow::Result<FullSak> {
    let sak = api::get_sak(saksnummer, kildesystem, true).await?;
    Ok(FullSak::from(sak))
}

pub async fn opprett_sak(data: NySak) -> anyhow::Result<SakResponse> {
    let sak = api::create_sak(ElementsSak::from(data)).await?;
    Ok(SakResponse::from(sak))
}
