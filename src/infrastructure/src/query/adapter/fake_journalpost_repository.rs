use async_trait::async_trait;
use domain::model::dokument::Dokument as DomainDokument;
use domain::model::journalpost::{Journalpost, JournalpostKey, JournalpostType, Journalpoststatus};

use application::query::services::hent_journalpost::JournalpostRepository;

#[derive(Clone, Debug, Default)]
pub struct FakeJournalpostRepository;

impl FakeJournalpostRepository {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl JournalpostRepository for FakeJournalpostRepository {
    async fn hent_journalpost(&self, key: JournalpostKey) -> Result<Journalpost, anyhow::Error> {
        Ok(Journalpost {
            client_reference: Some(match key {
                JournalpostKey::SkuffenId(id) => id,
                JournalpostKey::ArkivId(_) => uuid::Uuid::new_v4(),
            }),
            tittel: "Fake journalpost".to_string(),
            dokument_dato: "2026-01-01".to_string(),
            journalposttype: JournalpostType::InterntNotat,
            journalstatus: Journalpoststatus::Journalført,
            tilgang: None,
            saksbehandler: "Z00000".to_string(),
            dokumenter: vec![DomainDokument {
                client_reference: Some(uuid::Uuid::new_v4()),
                tittel: "Fake dokument".to_string(),
                filtype: "PDF".to_string(),
                dokument_referanse: Some(uuid::Uuid::new_v4()),
            }],
            journalpost_id: 10_000,
            kildesystem: Some("SKUFFEN".to_string()),
        })
    }
}
