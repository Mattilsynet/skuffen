use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId};
use domain::eksekvering::operasjon::{
    Operasjon, OperasjonId, OperasjonSammendrag, Operasjonsstatus,
};
use domain::eksekvering::tilstand::JournalpostTilstand;
use uuid::Uuid;

use crate::command::materialisering::Dekomponeringsplan;

/// Faktaendringen en vellykket operasjon medfører.
///
/// Skrives sammen med statusovergangen `sendt → ok` i én transaksjon
/// (SKU-0016 R4), slik at arkivsvaret og faktaene aldri kan divergere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Faktaoppdatering {
    Ingen,
    SakOpprettet {
        arkiv_id: String,
    },
    SakAvsluttet,
    SaksansvarligSatt {
        saksbehandler_id: String,
        saksbehandler_enhet: String,
    },
    DokumentRendret {
        dokument_id: SkuffenDokumentId,
        rendered_dokument_referanse: Uuid,
    },
    JournalpostOpprettet {
        journalpost_id: SkuffenJournalpostId,
        arkiv_id: String,
        /// Hoveddokumentet følger med opprettelsen og ligger dermed i arkivet.
        hoveddokument_id: SkuffenDokumentId,
    },
    VedleggArkivert {
        dokument_id: SkuffenDokumentId,
        arkiv_id: Option<String>,
    },
    JournalpostStatus {
        journalpost_id: SkuffenJournalpostId,
        tilstand: JournalpostTilstand,
    },
}

/// Foldet over `operasjon` (SKU-0016 R8). CommandStatus er ikke en kolonne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandOutcome {
    /// Minst én operasjon er ikke terminal, og ingen har feilet.
    Uavklart,
    /// Alle operasjoner er terminalt ok.
    Fullfort,
    /// Minst én operasjon er terminalt feilet. Monotont — kan ikke gå tilbake.
    Feilet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Gjenoppretting {
    /// `kjorer → klar`. Avbrutt før arkivkallet; trygt å prøve igjen.
    pub gjenopptatt: u64,
    /// `sendt → krever_avklaring`. Ukjent utfall (SKU-0016 R5).
    pub krever_avklaring: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dekomponeringsresultat {
    /// Antall operasjonsrader som faktisk ble satt inn. `0` betyr replay.
    pub nye_operasjoner: u64,
}

impl Dekomponeringsresultat {
    pub fn var_forste_gang(&self) -> bool {
        self.nye_operasjoner > 0
    }
}

/// Kommandoen en operasjon tilhører, med det statuspubliseringen trenger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandMetadata {
    pub command_id: Uuid,
    pub correlation_id: Option<Uuid>,
    pub command_type: domain::eksekvering::typer::CommandTypeCode,
    pub kontekst: domain::eksekvering::typer::Statuskontekst,
}

/// Bevis på at denne instansen er eneste executor.
///
/// Lederskap er noe man **holder**, ikke noe man sjekker én gang: låsen lever
/// nøyaktig så lenge denne verdien gjør. Droppes den, slippes låsen, og en
/// annen instans kan overta. Den må derfor eies av workeren gjennom hele dens
/// levetid.
pub trait ExecutorLease: Send + Sync {}

#[async_trait]
pub trait OperasjonRepository: Send + Sync {
    /// Forsøker å bli eneste executor. `None` betyr at en annen instans er
    /// leder nå — en normal, midlertidig tilstand, ikke en feil.
    async fn try_acquire_executor_lock(
        &self,
        executor_id: &str,
    ) -> Result<Option<Box<dyn ExecutorLease>>, anyhow::Error>;

    /// Persisterer en ferdig utregnet dekomponering: entitet, state og alle
    /// operasjonsrader, i én transaksjon.
    ///
    /// Regner ikke ut noe selv — operasjonslisten kommer fra domenets
    /// `dekomponer`. Idempotent via
    /// `UNIQUE (command_id, operasjonstype, entitet_id)`, så en replay setter
    /// inn null rader.
    async fn lagre_dekomponering(
        &self,
        plan: Dekomponeringsplan,
    ) -> Result<Dekomponeringsresultat, anyhow::Error>;

    /// Neste operasjon som er `klar`, eller `retry_venter` med forfalt frist.
    async fn hent_neste_kjorbare(&self) -> Result<Option<Operasjon>, anyhow::Error>;

    /// `klar|retry_venter → kjorer`. Returnerer nytt `attempt_no`.
    async fn marker_kjorer(
        &self,
        operasjon_id: OperasjonId,
        executor_id: &str,
    ) -> Result<i32, anyhow::Error>;

    /// `kjorer → sendt`, commitet **før** arkivkallet (SKU-0016 R4).
    async fn marker_sendt(
        &self,
        operasjon_id: OperasjonId,
        attempt_no: i32,
    ) -> Result<(), anyhow::Error>;

    /// `→ ok` med faktaoppdatering i samme transaksjon.
    async fn fullfor_ok(
        &self,
        operasjon_id: OperasjonId,
        attempt_no: i32,
        oppdatering: Faktaoppdatering,
    ) -> Result<(), anyhow::Error>;

    /// `AvventJournalfort` er ikke ferdig ennå: skriv observerte fakta og
    /// planlegg neste poll uten å gå terminalt (D20).
    async fn fullfor_poll(
        &self,
        operasjon_id: OperasjonId,
        attempt_no: i32,
        oppdatering: Faktaoppdatering,
        neste_forsok_at: DateTime<Utc>,
    ) -> Result<(), anyhow::Error>;

    async fn marker_retry(
        &self,
        operasjon_id: OperasjonId,
        attempt_no: i32,
        detalj: &str,
        neste_forsok_at: DateTime<Utc>,
    ) -> Result<(), anyhow::Error>;

    async fn marker_feilet(
        &self,
        operasjon_id: OperasjonId,
        attempt_no: i32,
        detalj: &str,
    ) -> Result<(), anyhow::Error>;

    /// `→ blokkert`. `blokkert_av` er derivert debughjelp, aldri autoritativ.
    async fn marker_blokkert(
        &self,
        operasjon_id: OperasjonId,
        attempt_no: Option<i32>,
        detalj: &str,
    ) -> Result<(), anyhow::Error>;

    async fn marker_klar(&self, operasjon_id: OperasjonId) -> Result<(), anyhow::Error>;

    /// Startup recovery: `kjorer → klar` og `sendt → krever_avklaring`.
    async fn gjenopprett_etter_restart(&self) -> Result<Gjenoppretting, anyhow::Error>;

    /// Kandidatene evalueringspasset skal se på.
    async fn hent_blokkerte(&self, grense: i64) -> Result<Vec<Operasjon>, anyhow::Error>;

    /// Operasjoner med ukjent utfall. Ikke kjørbare igjen — de venter på at et
    /// menneske rydder (SKU-0016 R5).
    async fn hent_krever_avklaring(&self) -> Result<Vec<Operasjon>, anyhow::Error>;

    /// Søskenoperasjoner på saken. Hentes kun for `AvsluttSak` (D4).
    async fn hent_sammendrag_for_sak(
        &self,
        sak_id: domain::eksekvering::id::SkuffenSakId,
    ) -> Result<Vec<OperasjonSammendrag>, anyhow::Error>;

    async fn hent_command_metadata(
        &self,
        operasjon_id: OperasjonId,
    ) -> Result<CommandMetadata, anyhow::Error>;

    async fn hent_status(
        &self,
        operasjon_id: OperasjonId,
    ) -> Result<Option<Operasjonsstatus>, anyhow::Error>;

    /// Foldet over kommandoens operasjoner.
    async fn hent_command_outcome(&self, command_id: Uuid)
    -> Result<CommandOutcome, anyhow::Error>;

    /// Operasjoner som ikke er terminale innen fristen (SKU-0016, D11).
    async fn hent_varselkandidater(
        &self,
        eldre_enn: DateTime<Utc>,
    ) -> Result<Vec<Operasjon>, anyhow::Error>;

    async fn marker_varslet(&self, operasjon_id: OperasjonId) -> Result<(), anyhow::Error>;
}
