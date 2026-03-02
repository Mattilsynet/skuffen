use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

use application::command::ports::command_state_port::{
    CommandStateError, CommandStateRepository, SakState as CommandSakState,
};
use application::command::ports::eksekvering_port::{
    ArkivGateway, OpprettJournalpostResultat, Utsendingsvalg,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

pub struct FakeArkivGatewayState {
    sak_counter: AtomicUsize,
    journalpost_counter: AtomicI32,
    dokument_counter: AtomicI32,
    avslutt_sak_calls: Mutex<Vec<String>>,
}

impl FakeArkivGatewayState {
    pub fn new() -> Self {
        Self {
            sak_counter: AtomicUsize::new(0),
            journalpost_counter: AtomicI32::new(0),
            dokument_counter: AtomicI32::new(0),
            avslutt_sak_calls: Mutex::new(Vec::new()),
        }
    }

    pub fn next_saksnummer(&self) -> String {
        let seq = self.sak_counter.fetch_add(1, Ordering::SeqCst) + 1;
        format!("2026/{:06}", 900000 + seq)
    }

    pub fn next_journalpost_id(&self) -> i32 {
        let seq = self.journalpost_counter.fetch_add(1, Ordering::SeqCst) + 1;
        10_000 + seq
    }

    pub fn next_dokument_id(&self) -> i32 {
        let seq = self.dokument_counter.fetch_add(1, Ordering::SeqCst) + 1;
        70_000 + seq
    }
}

#[derive(Clone)]
pub struct FakeArkivGateway {
    state: Arc<FakeArkivGatewayState>,
}

impl FakeArkivGateway {
    pub fn new(state: Arc<FakeArkivGatewayState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl ArkivGateway for FakeArkivGateway {
    async fn opprett_sak(
        &self,
        _command: &CommandEnvelope<Command>,
    ) -> Result<String, anyhow::Error> {
        Ok(self.state.next_saksnummer())
    }

    async fn opprett_journalpost(
        &self,
        _command: &CommandEnvelope<Command>,
        _saksnummer: &str,
        _utsending: Option<Utsendingsvalg>,
    ) -> Result<OpprettJournalpostResultat, anyhow::Error> {
        Ok(OpprettJournalpostResultat {
            journalpost_id: self.state.next_journalpost_id(),
        })
    }

    async fn legg_til_vedlegg(
        &self,
        _command: &CommandEnvelope<Command>,
        _journalpost_id: i32,
        dokument_ids: Vec<Uuid>,
    ) -> Result<Vec<Option<i32>>, anyhow::Error> {
        let arkiv_ids: Vec<i32> = dokument_ids
            .iter()
            .map(|_| self.state.next_dokument_id())
            .collect();
        Ok(arkiv_ids.into_iter().map(Some).collect())
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

    async fn avslutt_sak(&self, saksnummer: &str) -> Result<(), anyhow::Error> {
        self.state
            .avslutt_sak_calls
            .lock()
            .unwrap()
            .push(saksnummer.to_string());
        Ok(())
    }
}

pub struct FakeCommandStateRepository;

#[async_trait]
impl CommandStateRepository for FakeCommandStateRepository {
    async fn hent_sak_state(
        &self,
        _saksnummer: &str,
    ) -> Result<CommandSakState, CommandStateError> {
        Ok(CommandSakState { avsluttet: false })
    }
}
