use async_trait::async_trait;
use domain::eksekvering::typer::CommandLifecycleEvent;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Utsendingsvalg {
    MedUtsending,
    UtenUtsending,
}

#[derive(Debug, Clone)]
pub struct OpprettJournalpostResultat {
    pub journalpost_id: i32,
}

#[async_trait]
pub trait ArkivGateway: Send + Sync {
    async fn opprett_sak(
        &self,
        command: &CommandEnvelope<Command>,
    ) -> Result<String, anyhow::Error>;

    async fn opprett_journalpost(
        &self,
        command: &CommandEnvelope<Command>,
        saksnummer: &str,
        utsending: Option<Utsendingsvalg>,
    ) -> Result<OpprettJournalpostResultat, anyhow::Error>;

    async fn legg_til_vedlegg(
        &self,
        command: &CommandEnvelope<Command>,
        journalpost_id: i32,
        dokument_ids: Vec<uuid::Uuid>,
    ) -> Result<Vec<Option<i32>>, anyhow::Error>;

    async fn sett_journalpost_status(
        &self,
        journalpost_id: i32,
        status: &str,
    ) -> Result<(), anyhow::Error>;

    async fn avskriv_journalpost(
        &self,
        journalpost_id: i32,
        avskrivingsmaate: &str,
    ) -> Result<(), anyhow::Error>;

    async fn avslutt_sak(&self, saksnummer: &str) -> Result<(), anyhow::Error>;
}

#[async_trait]
pub trait EksekveringKvitteringPublisher: Send + Sync {
    async fn publiser_done(
        &self,
        subject: &str,
        command: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error>;
}

#[async_trait]
pub trait EksekveringStatusPublisher: Send + Sync {
    async fn publiser_status(&self, event: CommandLifecycleEvent) -> Result<(), anyhow::Error>;
}
