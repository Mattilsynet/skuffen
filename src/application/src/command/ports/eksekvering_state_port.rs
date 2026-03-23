use async_trait::async_trait;
use chrono::{DateTime, Utc};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EksekveringStatus {
    Pending,
    Running,
    Ok,
    Blocked,
    Error,
    Retrying,
}

#[derive(Debug, Clone)]
pub struct EksekveringKommando {
    pub command_id: Uuid,
    pub envelope: CommandEnvelope<Command>,
    pub attempts: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SakStatus {
    UnderBehandling,
    Ferdig,
    Avsluttet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SakState {
    pub status: SakStatus,
    pub opprettet: bool,
    pub saksnummer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalpostState {
    pub journalfoert: bool,
    pub avskrevet: bool,
    pub ekspedert: bool,
    pub har_feilede_dokumenter: bool,
    pub med_utsending: bool,
    pub journalposttype: char,
    pub journalpostnummer: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DokumentState {
    pub lagt_til: bool,
    pub irrecoverable_feil: bool,
}

#[async_trait]
pub trait EksekveringStateRepository: Send + Sync {
    async fn hent_sak_state_fra_state(&self, sak_id: Uuid) -> Result<Option<SakState>, anyhow::Error>;
    async fn lagre_sak_state(&self, sak_id: Uuid, state: SakState) -> Result<(), anyhow::Error>;

    async fn hent_journalpost_state_fra_state(
        &self,
        journalpost_id: Uuid,
    ) -> Result<Option<JournalpostState>, anyhow::Error>;
    async fn lagre_journalpost_state(
        &self,
        journalpost_id: Uuid,
        sak_id: Uuid,
        state: JournalpostState,
    ) -> Result<(), anyhow::Error>;

    async fn hent_journalposter_for_sak_fra_state(
        &self,
        sak_id: Uuid,
    ) -> Result<Vec<JournalpostState>, anyhow::Error>;

    async fn hent_dokument_state_fra_state(
        &self,
        dokument_id: Uuid,
    ) -> Result<Option<DokumentState>, anyhow::Error>;
    async fn lagre_dokument_state(
        &self,
        dokument_id: Uuid,
        journalpost_id: Uuid,
        state: DokumentState,
    ) -> Result<(), anyhow::Error>;

    async fn oppdater_eksekvering(
        &self,
        command_id: Uuid,
        status: EksekveringStatus,
        last_error: Option<String>,
        next_retry_at: Option<DateTime<Utc>>,
    ) -> Result<(), anyhow::Error>;

    async fn registrer_kommando(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error>;

    async fn hent_klare_kommandoer(
        &self,
        limit: i64,
        worker_id: &str,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error>;
}
