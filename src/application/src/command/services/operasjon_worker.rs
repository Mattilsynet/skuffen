use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use domain::eksekvering::typer::{Operasjonshendelse, Operasjonstatus, StatusErrorCode};

use crate::command::ports::{
    operasjon_port::OperasjonRepository, status_publisher_port::StatusPublisher,
};
use crate::command::services::eksekver_operasjon::EksekverOperasjonService;
use crate::command::services::evaluer_operasjoner::EvaluerOperasjonerService;

#[derive(Debug, Clone, Copy)]
pub struct WorkerInnstillinger {
    /// Hvor ofte det advisory 24-timersvarselet sjekkes.
    pub varselintervall: Duration,
    /// Hvor lenge workeren sover når køen er tom.
    pub tomgangspause: Duration,
    /// Hvor lenge en operasjon kan være ikke-terminal før den varsles (D11).
    pub varselfrist: chrono::Duration,
    /// Hvor mange operasjoner ett evalueringspass ser på.
    pub evalueringsgrense: i64,
}

impl Default for WorkerInnstillinger {
    fn default() -> Self {
        Self {
            varselintervall: Duration::from_secs(30),
            tomgangspause: Duration::from_secs(2),
            varselfrist: chrono::Duration::hours(24),
            evalueringsgrense: 200,
        }
    }
}

/// Kjører operasjoner til køen er tom, evaluerer blokkerte periodisk, og
/// emitter det advisory 24-timersvarselet.
///
/// Én aktiv executor, håndhevet med advisory lock.
pub struct OperasjonWorker {
    executor: EksekverOperasjonService,
    evaluator: EvaluerOperasjonerService,
    operasjon: Arc<dyn OperasjonRepository>,
    publisher: Arc<dyn StatusPublisher>,
    executor_id: String,
    innstillinger: WorkerInnstillinger,
}

impl OperasjonWorker {
    pub fn new(
        executor: EksekverOperasjonService,
        evaluator: EvaluerOperasjonerService,
        operasjon: Arc<dyn OperasjonRepository>,
        publisher: Arc<dyn StatusPublisher>,
        executor_id: impl Into<String>,
        innstillinger: WorkerInnstillinger,
    ) -> Self {
        Self {
            executor,
            evaluator,
            operasjon,
            publisher,
            executor_id: executor_id.into(),
            innstillinger,
        }
    }

    /// Tar eierskap og kjører til prosessen stoppes.
    pub async fn run(&self) -> Result<()> {
        if !self
            .operasjon
            .try_acquire_executor_lock(&self.executor_id)
            .await?
        {
            // En annen instans eier køen. Ikke en feil.
            return Ok(());
        }

        // Startup recovery før noe annet: `kjorer → klar` er trygt å prøve
        // igjen, `sendt → krever_avklaring` har ukjent utfall (SKU-0016 R5).
        let gjenoppretting = self.operasjon.gjenopprett_etter_restart().await?;
        if gjenoppretting.krever_avklaring > 0 {
            self.varsle_krever_avklaring().await?;
        }

        let mut siste_varsel = std::time::Instant::now();

        loop {
            // Evalueringspasset *er* readiness-mekanismen: en nydekomponert
            // operasjon står `blokkert` til et pass flytter den til `klar`.
            self.evaluator.run_evaluation_pass().await?;

            // Tøm køen før vi sover. Hver fullførte operasjon kan gjøre søsken
            // kjørbare, så vi evaluerer på nytt mellom hver.
            let mut arbeidet = false;
            while self.executor.run_next().await? {
                arbeidet = true;
                self.evaluator.run_evaluation_pass().await?;
            }

            if siste_varsel.elapsed() >= self.innstillinger.varselintervall {
                self.emitter_varsler().await?;
                siste_varsel = std::time::Instant::now();
            }

            if !arbeidet {
                tokio::time::sleep(self.innstillinger.tomgangspause).await;
            }
        }
    }

    /// Ett pass, for tester og for kall utenfra.
    pub async fn run_once(&self) -> Result<bool> {
        let arbeidet = self.executor.run_next().await?;
        self.evaluator.run_evaluation_pass().await?;
        Ok(arbeidet)
    }

    /// Advisory varsel. Avbryter ingenting og gjør ingen operasjon terminal —
    /// den fortsetter å prøve (D11).
    async fn emitter_varsler(&self) -> Result<()> {
        let frist = Utc::now() - self.innstillinger.varselfrist;
        for op in self.operasjon.hent_varselkandidater(frist).await? {
            let command = self
                .operasjon
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

            self.operasjon.marker_varslet(op.operasjon_id).await?;
        }
        Ok(())
    }

    /// Publiserer operasjoner som kom ut av recovery med ukjent utfall.
    ///
    /// Disse er ikke kjørbare igjen. Uten et event utad ville de blitt
    /// usynlige rader som ingen leter etter (SKU-0016 R5).
    async fn varsle_krever_avklaring(&self) -> Result<()> {
        for op in self.operasjon.hent_krever_avklaring().await? {
            let command = self
                .operasjon
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
                    "Utfallet er ukjent og må avklares manuelt.",
                    Some(StatusErrorCode::ProcessingFailed),
                ))
                .await?;
        }
        Ok(())
    }
}
