use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use domain::eksekvering::typer::{
    CommandEvent, CommandStatus, Operasjonshendelse, Operasjonstatus, StatusErrorCode,
};

use tokio_util::sync::CancellationToken;

use crate::command::ports::{
    operasjon_port::{CommandMetadata, CommandOutcome, ExecutorLease, OperasjonRepository},
    status_publisher_port::StatusPublisher,
};
use crate::command::services::eksekver_operasjon::EksekverOperasjonService;

#[derive(Debug, Clone, Copy)]
pub struct WorkerInnstillinger {
    /// Hvor ofte det advisory 24-timersvarselet sjekkes.
    pub varselintervall: Duration,
    /// Hvor lenge workeren sover når køen er tom.
    pub tomgangspause: Duration,
    /// Hvor ofte en instans som ikke er leder prøver å overta.
    pub lederforsok_intervall: Duration,
    /// Hvor lenge en operasjon kan være ikke-terminal før den varsles (D11).
    pub varselfrist: chrono::Duration,
}

impl Default for WorkerInnstillinger {
    fn default() -> Self {
        Self {
            varselintervall: Duration::from_secs(30),
            tomgangspause: Duration::from_secs(2),
            lederforsok_intervall: Duration::from_secs(5),
            varselfrist: chrono::Duration::hours(24),
        }
    }
}

/// Klientvendt tekst for et utfall som må ryddes manuelt. Samme melding på
/// operasjons- og kommandonivå, så de to strømmene ikke forteller ulike ting.
const UAVKLART_MELDING: &str = "Utfallet er ukjent og må avklares manuelt.";

/// Drenerer forfalte operasjoner og emitter det advisory 24-timersvarselet.
///
/// Utfall avgjøres ett sted, i executoren (SKU-0020 R1). Workeren plukker
/// ingenting selv; den kaller `run_next` til køen er tom.
///
/// Én aktiv executor, håndhevet med advisory lock.
pub struct OperasjonWorker {
    executor: EksekverOperasjonService,
    operasjon_repo: Arc<dyn OperasjonRepository>,
    publisher: Arc<dyn StatusPublisher>,
    executor_id: String,
    innstillinger: WorkerInnstillinger,
    shutdown: CancellationToken,
}

impl OperasjonWorker {
    pub fn new(
        executor: EksekverOperasjonService,
        operasjon_repo: Arc<dyn OperasjonRepository>,
        publisher: Arc<dyn StatusPublisher>,
        executor_id: impl Into<String>,
        innstillinger: WorkerInnstillinger,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            executor,
            operasjon_repo,
            publisher,
            executor_id: executor_id.into(),
            innstillinger,
            shutdown,
        }
    }

    /// Feil propagerer til supervisoren, som restarter med backoff. Leasen
    /// slippes da, så en annen instans kan overta i mellomtiden.
    pub async fn run(&self) -> Result<()> {
        let Some(_lease) = self.vent_paa_lederskap().await? else {
            return Ok(());
        };

        // Kjøres før løkka. `kjorer` betyr avbrutt før arkivkallet og er trygt
        // å prøve igjen. `sendt` betyr at kallet gikk ut med ukjent utfall, og
        // må avklares av et menneske (SKU-0016 R5).
        let gjenoppretting = self.operasjon_repo.gjenopprett_etter_restart().await?;
        tracing::info!(
            executor_id = %self.executor_id,
            gjenopptatt = gjenoppretting.gjenopptatt,
            krever_avklaring = gjenoppretting.krever_avklaring,
            "executor overtok lederskapet"
        );
        // Ubetinget: spørringen er selvbegrensende på `avklaring_varslet_at`,
        // så en krasj mellom recovery-commit og publisering ikke etterlater
        // stille rader (SKU-0020 R6).
        self.varsle_krever_avklaring().await?;

        let mut siste_varsel = std::time::Instant::now();

        loop {
            if self.shutdown.is_cancelled() {
                return Ok(());
            }

            let mut arbeidet = false;
            while self.executor.run_next().await? {
                arbeidet = true;

                // Avbrytes en operasjon etter `sendt`, er utfallet ukjent og
                // må ryddes manuelt. Derfor mellom operasjoner, aldri under.
                if self.shutdown.is_cancelled() {
                    return Ok(());
                }
            }

            if siste_varsel.elapsed() >= self.innstillinger.varselintervall {
                self.emitter_varsler().await?;
                siste_varsel = std::time::Instant::now();
            }

            if !arbeidet {
                self.sov(self.innstillinger.tomgangspause).await;
            }
        }
    }

    /// Venter til denne instansen blir eneste executor. Ved utrulling starter
    /// ny instans mens den gamle fortsatt holder låsen, og overtar når den
    /// slipper den.
    ///
    /// `None` betyr at nedstenging kom først.
    async fn vent_paa_lederskap(&self) -> Result<Option<Box<dyn ExecutorLease>>> {
        loop {
            if self.shutdown.is_cancelled() {
                return Ok(None);
            }

            if let Some(lease) = self
                .operasjon_repo
                .try_acquire_executor_lock(&self.executor_id)
                .await?
            {
                return Ok(Some(lease));
            }

            self.sov(self.innstillinger.lederforsok_intervall).await;
        }
    }

    /// Våkner umiddelbart ved nedstenging.
    async fn sov(&self, varighet: Duration) {
        tokio::select! {
            _ = tokio::time::sleep(varighet) => {}
            _ = self.shutdown.cancelled() => {}
        }
    }

    /// Advisory varsel. Avbryter ingenting og gjør ingen operasjon terminal —
    /// den fortsetter å prøve (D11).
    async fn emitter_varsler(&self) -> Result<()> {
        let frist = Utc::now() - self.innstillinger.varselfrist;
        for op in self.operasjon_repo.hent_varselkandidater(frist).await? {
            let command = self
                .operasjon_repo
                .hent_command_metadata(op.operasjon_id)
                .await?;

            self.publisher
                .publiser_operasjonstatus(Operasjonstatus::new(
                    command.command_id,
                    command.correlation_id,
                    op.operasjon_id,
                    op.operasjonstype,
                    Operasjonshendelse::Varsel,
                    0,
                    "Operasjonen har ikke fullført innen fristen. Forsøkene fortsetter.",
                    Some(StatusErrorCode::PrerequisitePending),
                ))
                .await?;

            self.operasjon_repo.marker_varslet(op.operasjon_id).await?;
        }
        Ok(())
    }

    /// Publiserer operasjoner som kom ut av recovery med ukjent utfall.
    ///
    /// Disse er ikke kjørbare igjen. Uten et event utad ville de blitt
    /// usynlige rader som ingen leter etter (SKU-0016 R5).
    ///
    /// Både operasjons- og kommandonivået varsles. En klient som følger
    /// `arkiv.status.<cmd>.command` — den anbefalte subscriptionen for «bare
    /// utfallet» — ville ellers sett stillhet, fordi `krever_avklaring` ikke
    /// er en terminal operasjonshendelse og aldri når folden via executoren.
    ///
    /// Markeringen skjer etter publiseringen. En krasj imellom gir ett
    /// duplikat, ikke tap — statusstrømmen er at-least-once (SKU-0020 R5).
    async fn varsle_krever_avklaring(&self) -> Result<()> {
        for op in self.operasjon_repo.hent_krever_avklaring().await? {
            let command = self
                .operasjon_repo
                .hent_command_metadata(op.operasjon_id)
                .await?;

            self.publisher
                .publiser_operasjonstatus(Operasjonstatus::new(
                    command.command_id,
                    command.correlation_id,
                    op.operasjon_id,
                    op.operasjonstype,
                    Operasjonshendelse::KreverAvklaring,
                    0,
                    UAVKLART_MELDING,
                    Some(StatusErrorCode::ProcessingFailed),
                ))
                .await?;

            self.publiser_uavklart_command(&command).await?;

            self.operasjon_repo
                .marker_avklaring_varslet(op.operasjon_id)
                .await?;
        }
        Ok(())
    }

    /// Publiseres kun når foldet faktisk står på `KreverAvklaring`. Har en
    /// søskenoperasjon allerede feilet terminalt, er kommandoen `Feilet`, og
    /// det utfallet kan ikke trekkes tilbake.
    async fn publiser_uavklart_command(&self, command: &CommandMetadata) -> Result<()> {
        if self
            .operasjon_repo
            .hent_command_outcome(command.command_id)
            .await?
            != CommandOutcome::KreverAvklaring
        {
            return Ok(());
        }

        self.publisher
            .publiser_command_status(CommandStatus::new(
                command.command_id,
                command.correlation_id,
                command.command_type,
                CommandEvent::KreverAvklaring,
                UAVKLART_MELDING,
                Some(StatusErrorCode::ProcessingFailed),
                command.kontekst.clone(),
            ))
            .await
    }
}
