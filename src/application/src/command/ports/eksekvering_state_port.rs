use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::plan::JournalpostType;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use uuid::Uuid;

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
pub struct SakTransition {
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
    pub journalposttype: JournalpostType,
    pub journalpostnummer: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DokumentState {
    pub lagt_til: bool,
    pub irrecoverable_feil: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalpostOpprettetTransition {
    pub journalpostnummer: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalpostOvergangVedJournalfoering {
    pub journalfoert: bool,
    pub ekspedert: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SakStateRegistration {
    pub sak_id: SkuffenSakId,
    pub state: SakState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalpostStateRegistration {
    pub journalpost_id: SkuffenJournalpostId,
    pub sak_id: SkuffenSakId,
    pub state: JournalpostState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EksekveringssystemRegistration {
    pub sak: Option<SakStateRegistration>,
    pub journalpost: Option<JournalpostStateRegistration>,
}

#[async_trait]
pub trait EksekveringStateRepository: Send + Sync {
    async fn hent_sak_state(&self, sak_id: SkuffenSakId)
        -> Result<Option<SakState>, anyhow::Error>;
    async fn ensure_sak_state(
        &self,
        sak_id: SkuffenSakId,
        state: SakState,
    ) -> Result<SakState, anyhow::Error>;
    async fn anvend_sak_transition(
        &self,
        sak_id: SkuffenSakId,
        transition: SakTransition,
    ) -> Result<SakState, anyhow::Error>;

    async fn hent_journalpost_state(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<JournalpostState>, anyhow::Error>;
    async fn ensure_journalpost_state(
        &self,
        journalpost_id: SkuffenJournalpostId,
        sak_id: SkuffenSakId,
        state: JournalpostState,
    ) -> Result<JournalpostState, anyhow::Error>;
    async fn anvend_journalpost_opprettet(
        &self,
        journalpost_id: SkuffenJournalpostId,
        transition: JournalpostOpprettetTransition,
    ) -> Result<JournalpostState, anyhow::Error>;
    async fn anvend_journalpost_overgang_ved_journalfoering(
        &self,
        journalpost_id: SkuffenJournalpostId,
        transition: JournalpostOvergangVedJournalfoering,
    ) -> Result<JournalpostState, anyhow::Error>;
    async fn anvend_journalpost_avskrevet(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<JournalpostState, anyhow::Error>;

    async fn hent_journalposter_for_sak(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Vec<JournalpostState>, anyhow::Error>;

    async fn hent_dokument_state(
        &self,
        dokument_id: SkuffenDokumentId,
    ) -> Result<Option<DokumentState>, anyhow::Error>;
    async fn ensure_dokument_state(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
        state: DokumentState,
    ) -> Result<DokumentState, anyhow::Error>;
    async fn anvend_dokument_lagt_til(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<DokumentState, anyhow::Error>;

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
    ) -> Result<bool, anyhow::Error>;

    async fn ensure_registrert_i_eksekveringssystem(
        &self,
        registration: &EksekveringssystemRegistration,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<EksekveringsregistreringResultat, anyhow::Error> {
        if let Some(sak) = &registration.sak {
            let _ = self.ensure_sak_state(sak.sak_id, sak.state.clone()).await?;
        }

        if let Some(journalpost) = &registration.journalpost {
            let _ = self
                .ensure_journalpost_state(
                    journalpost.journalpost_id,
                    journalpost.sak_id,
                    journalpost.state.clone(),
                )
                .await?;
        }

        Ok(if self.registrer_kommando(envelope).await? {
            EksekveringsregistreringResultat::Nyregistrert
        } else {
            EksekveringsregistreringResultat::EksisterteUtenVenterPublisert
        })
    }

    async fn marker_utfores_venter_publisert(&self, command_id: Uuid) -> Result<(), anyhow::Error>;

    async fn hent_klare_kommandoer(
        &self,
        limit: i64,
        worker_id: &str,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error>;
}
