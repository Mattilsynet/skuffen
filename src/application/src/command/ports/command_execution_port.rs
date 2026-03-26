use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::eksekvering::execution::{EksekveringStatus, Ventegrunn};
use domain::eksekvering::id::{SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::typer::CommandTypeCode;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use uuid::Uuid;

use crate::command::ports::execution_registration_port::EksekveringssystemRegistration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EksekveringsregistreringResultat {
    Nyregistrert,
    EksisterteUtenVenterPublisert,
    EksisterteMedVenterPublisert,
}

impl EksekveringsregistreringResultat {
    pub fn skal_publisere_utfores_venter(self) -> bool {
        matches!(
            self,
            Self::Nyregistrert | Self::EksisterteUtenVenterPublisert
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NyKommandoEksekvering {
    pub envelope: CommandEnvelope<Command>,
    pub command_type: CommandTypeCode,
    pub sak_id: Option<SkuffenSakId>,
    pub journalpost_id: Option<SkuffenJournalpostId>,
    pub status: EksekveringStatus,
    pub ventegrunn: Option<Ventegrunn>,
    pub last_detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EksekveringKommando {
    pub command_id: Uuid,
    pub envelope: CommandEnvelope<Command>,
    pub attempt_no: i32,
    pub utfores_venter_publisert: bool,
}

#[async_trait]
pub trait CommandExecutionRepository: Send + Sync {
    async fn try_acquire_executor_lock(&self, executor_id: &str) -> Result<bool, anyhow::Error>;

    async fn opprett(
        &self,
        registration: &EksekveringssystemRegistration,
        ny: NyKommandoEksekvering,
    ) -> Result<EksekveringsregistreringResultat, anyhow::Error>;

    async fn marker_utfores_venter_publisert(&self, command_id: Uuid) -> Result<(), anyhow::Error>;

    async fn hent_neste_kjorbare(&self) -> Result<Option<EksekveringKommando>, anyhow::Error>;

    async fn marker_kjorer(&self, command_id: Uuid) -> Result<i32, anyhow::Error>;

    async fn registrer_forsok(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        executor_id: &str,
    ) -> Result<(), anyhow::Error>;

    async fn marker_ok(&self, command_id: Uuid, attempt_no: i32) -> Result<(), anyhow::Error>;

    async fn marker_retry_venter(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        detalj: &str,
        retry_ready_at: DateTime<Utc>,
    ) -> Result<(), anyhow::Error>;

    async fn marker_venter(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        grunn: &Ventegrunn,
        detalj: &str,
    ) -> Result<(), anyhow::Error>;

    async fn marker_feil(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        detalj: &str,
    ) -> Result<(), anyhow::Error>;

    async fn marker_forsok_avbrutt(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        detalj: &str,
    ) -> Result<(), anyhow::Error>;

    async fn hent_ventende_for_sak(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error>;

    async fn hent_ventende_for_journalpost(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error>;

    async fn oppdater_til_klar(&self, command_id: Uuid) -> Result<(), anyhow::Error>;

    async fn oppdater_venter(
        &self,
        command_id: Uuid,
        grunn: &Ventegrunn,
        detalj: &str,
    ) -> Result<(), anyhow::Error>;

    async fn oppdater_til_feil(&self, command_id: Uuid, detalj: &str) -> Result<(), anyhow::Error>;

    async fn reset_kjorer_til_klar(&self) -> Result<u64, anyhow::Error>;
}
