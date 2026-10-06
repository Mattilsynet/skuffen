use async_trait::async_trait;
use chrono::NaiveDate;
use domain::model::dokument::{ArkivDokumentId, Dokument as DomainDokument};
use domain::model::journalpost::{Journalpost, JournalpostKey, JournalpostType, Journalpoststatus};

use application::query::services::hent_journalpost::JournalpostRepository;

#[derive(Clone, Debug, Default)]
pub struct FakeJournalpostRepository;

impl FakeJournalpostRepository {
    pub fn new() -> Self {
        Self
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
            dokument_dato: NaiveDate::from_ymd_opt(2026, 1, 1)
                .and_then(|dato| dato.and_hms_opt(0, 0, 0))
                .ok_or_else(|| anyhow::anyhow!("ugyldig fake-dato"))?,
            journalposttype: JournalpostType::InterntNotat,
            journalstatus: Journalpoststatus::Journalført,
            tilgang: None,
            saksbehandler: Some("Z00000".to_string()),
            saksbehandler_enhet: Some("42".to_string()),
            dokumenter: vec![DomainDokument {
                dokument_id: ArkivDokumentId(20_000),
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
