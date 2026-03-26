use async_trait::async_trait;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::plan::JournalpostType;

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

#[async_trait]
pub trait EksekveringSnapshotRepository: Send + Sync {
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
    async fn anvend_dokument_irrecoverable_feil(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<DokumentState, anyhow::Error>;
}
