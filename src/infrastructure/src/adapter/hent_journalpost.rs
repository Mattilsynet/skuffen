// use anyhow::anyhow;
// use application::services::hent_journalpost::JournalpostRepository;
// use async_trait::async_trait;
// use lib_schemas::arkiv::v2::journalpost::{JournalpostId, JournalpostKey, JournalpostResponse};
//
// pub struct SikriRepository;
//
// #[async_trait]
// impl JournalpostRepository for SikriRepository {
//     async fn hent_journalpost(
//         &self,
//         key: JournalpostKey,
//     ) -> Result<JournalpostResponse, anyhow::Error> {
//         let journalpost_id: JournalpostId = match key {
//             JournalpostKey::SkuffenId(_uuid) => {
//                 return Err(anyhow!("Har ikke implementert skuffen id enda."))
//             }
//             JournalpostKey::ArkivId(journalpost_id) => journalpost_id,
//         };
//         let sak_reponse = sikri_client::journalpost(journalpost_id.as_str(), "SKUFFEN").await?;
//         Ok(sak_reponse.into())
//     }
// }
