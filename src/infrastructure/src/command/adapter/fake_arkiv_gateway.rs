use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};

use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

use application::command::ports::eksekvering_port::{
    ArkivGateway, OpprettJournalpostResultat, Utsendingsvalg,
};

#[derive(Clone, Default)]
pub struct FakeArkivGateway {
    sak_counter: Arc<AtomicUsize>,
    journalpost_counter: Arc<AtomicI32>,
    dokument_counter: Arc<AtomicI32>,
}

impl FakeArkivGateway {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ArkivGateway for FakeArkivGateway {
    async fn opprett_sak(
        &self,
        _command: &CommandEnvelope<Command>,
    ) -> Result<String, anyhow::Error> {
        let seq = self.sak_counter.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(format!("2026/{:06}", 900000 + seq))
    }

    async fn opprett_journalpost(
        &self,
        _command: &CommandEnvelope<Command>,
        _saksnummer: &str,
        _utsending: Option<Utsendingsvalg>,
    ) -> Result<OpprettJournalpostResultat, anyhow::Error> {
        let seq = self.journalpost_counter.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(OpprettJournalpostResultat {
            journalpost_id: 10_000 + seq,
        })
    }

    async fn legg_til_vedlegg(
        &self,
        _command: &CommandEnvelope<Command>,
        _journalpost_id: i32,
        dokument_ids: Vec<uuid::Uuid>,
    ) -> Result<Vec<Option<i32>>, anyhow::Error> {
        let mut arkiv_ids = Vec::with_capacity(dokument_ids.len());
        for _ in dokument_ids {
            let seq = self.dokument_counter.fetch_add(1, Ordering::SeqCst) + 1;
            arkiv_ids.push(Some(70_000 + seq));
        }
        Ok(arkiv_ids)
    }

    async fn sett_journalpost_status(
        &self,
        _journalpost_id: i32,
        _status: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn avskriv_journalpost(
        &self,
        _journalpost_id: i32,
        _avskrivingsmaate: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn avslutt_sak(&self, _saksnummer: &str) -> Result<(), anyhow::Error> {
        Ok(())
    }
}
