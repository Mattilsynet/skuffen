use async_trait::async_trait;
use domain::model::journalpost::{Journalpost, JournalpostKey};

use application::query::services::hent_journalpost::JournalpostRepository;

/// Produksjonsadapter inntil ekte journalpost-backing finnes. Returnerer en
/// tydelig feil i stedet for fake OK-data, slik at ekte klienter aldri får
/// syntetiske svar som ser gyldige ut.
#[derive(Clone, Debug, Default)]
pub struct NotImplementedJournalpostRepository;

impl NotImplementedJournalpostRepository {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl JournalpostRepository for NotImplementedJournalpostRepository {
    async fn hent_journalpost(&self, _key: JournalpostKey) -> Result<Journalpost, anyhow::Error> {
        Err(anyhow::anyhow!("hent_journalpost er ikke implementert"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::model::journalpost::JournalpostId;

    #[tokio::test]
    async fn returnerer_feil_ikke_fake_data() {
        let repo = NotImplementedJournalpostRepository::new();
        let resultat = repo
            .hent_journalpost(JournalpostKey::ArkivId(JournalpostId(
                "2026/1-1".to_string(),
            )))
            .await;
        assert!(resultat.is_err());
    }
}
