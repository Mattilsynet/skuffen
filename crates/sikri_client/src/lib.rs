mod api;
pub mod domain;
pub mod dto;
mod secret;

use crate::domain::ny_sak::NySak;
use crate::domain::sak::SakRespons;
use crate::dto::elements_dokument::ElementsDokument;
use crate::dto::elements_dokument_response::ElementsDokumentRespons;
use crate::dto::elements_journalpost::{ElementsJournalpost, ElementsJournalpostRespons};
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

pub async fn opprett_journalpost(
    journalpost: ElementsJournalpost,
    saksnummer: &str,
) -> anyhow::Result<ElementsJournalpostRespons> {
    api::opprett_journalpost(journalpost, saksnummer).await
}

pub async fn legg_til_vedlegg(
    journalpost_id: i32,
    dokumenter: Vec<ElementsDokument>,
) -> anyhow::Result<Vec<ElementsDokumentRespons>> {
    api::legg_til_vedlegg(journalpost_id, dokumenter).await
}

pub async fn sett_journalpost_status(journalpost_id: i32, status: &str) -> anyhow::Result<()> {
    api::sett_journalpost_status(journalpost_id, status).await
}

pub async fn avskriv_journalpost(
    journalpost_id: i32,
    avskrivingsmaate: &str,
) -> anyhow::Result<()> {
    api::avskriv_journalpost(journalpost_id, avskrivingsmaate).await
}

pub async fn avslutt_sak(saksnummer: &str) -> anyhow::Result<()> {
    api::avslutt_sak(saksnummer).await
}
