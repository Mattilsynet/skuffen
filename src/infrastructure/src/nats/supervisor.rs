use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::http::helse::Helse;

/// Fem forsøk, rullende: `stable_run_window` nullstiller telleren, så en task
/// som feiler sjelden over lang tid dør ikke av akkumulerte restarter
/// (SKU-0021 R2).
pub const RESTARTBUDSJETT: u32 = 5;

/// Tasknavnene, ett sted. Supervisoren melder tasken inn i readiness under
/// dette navnet, og oppstarten registrerer den samme lista på forhånd — så en
/// omdøpt task kan ikke stille falle ut av aggregatet.
pub mod tasknavn {
    pub const QUERY_LISTENER: &str = "query_listener";
    pub const READY_REPLIER: &str = "ready_replier";
    pub const MEDIA_LISTENER: &str = "media_listener";
    pub const COMMAND_LISTENER: &str = "command_listener";
    pub const VALIDATION_LISTENER: &str = "validation_listener";
    pub const ADMIN_LISTENER: &str = "admin_listener";
    pub const DEKOMPONERING_LISTENER: &str = "dekomponering_listener";
    pub const EXECUTION_WORKER: &str = "execution_worker";

    /// Alle tasks som kjører under supervisor.
    pub const ALLE: [&str; 8] = [
        QUERY_LISTENER,
        READY_REPLIER,
        MEDIA_LISTENER,
        COMMAND_LISTENER,
        VALIDATION_LISTENER,
        ADMIN_LISTENER,
        DEKOMPONERING_LISTENER,
        EXECUTION_WORKER,
    ];
}

pub struct TaskSupervisor {
    name: String,
    initial_backoff: Duration,
    max_backoff: Duration,
    stable_run_window: Duration,
    max_restart_attempts: Option<u32>,
    shutdown: Option<tokio_util::sync::CancellationToken>,
    /// Tasken sitt readiness-flagg, satt mens `run_once` kjører.
    oppe: Option<Arc<AtomicBool>>,
    /// Restarter siden oppstart. Tallet finnes allerede som `attempt`; her er
    /// det eksponert, slik at en metrikk senere blir en tilføyelse.
    restarter: Arc<AtomicU64>,
}

impl TaskSupervisor {
    fn er_nedstengt(&self) -> bool {
        self.shutdown
            .as_ref()
            .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
    }

    fn sett_oppe(&self, oppe: bool) {
        if let Some(flagg) = &self.oppe {
            flagg.store(oppe, Ordering::Relaxed);
        }
    }

    pub fn critical(name: impl Into<String>, max_restart_attempts: u32) -> Self {
        Self {
            name: name.into(),
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            stable_run_window: Duration::from_secs(30),
            max_restart_attempts: Some(max_restart_attempts),
            shutdown: None,
            oppe: None,
            restarter: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn background(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            stable_run_window: Duration::from_secs(30),
            max_restart_attempts: None,
            shutdown: None,
            oppe: None,
            restarter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Skiller «stoppet fordi den skal» fra «stoppet uventet», slik at en task
    /// som avslutter ved SIGTERM ikke restartes.
    pub fn with_shutdown(mut self, shutdown: tokio_util::sync::CancellationToken) -> Self {
        self.shutdown = Some(shutdown);
        self
    }

    /// Melder tasken inn i readiness-aggregatet (SKU-0021 R5).
    pub fn with_helse(mut self, helse: &Helse) -> Self {
        self.oppe = Some(helse.registrer_task(&self.name));
        self
    }

    pub fn restarter(&self) -> u64 {
        self.restarter.load(Ordering::Relaxed)
    }

    pub async fn run<F, Fut>(&self, mut run_once: F) -> anyhow::Result<()>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = anyhow::Result<()>>,
    {
        let mut attempt: u32 = 0;
        let mut backoff = self.initial_backoff;

        loop {
            if self.er_nedstengt() {
                self.sett_oppe(false);
                return Ok(());
            }

            let started_at = Instant::now();
            self.sett_oppe(true);
            let result = run_once().await;
            self.sett_oppe(false);
            let run_duration = started_at.elapsed();

            if self.er_nedstengt() {
                // Nedstenging er en normal slutt. En feil som oppsto mens
                // tasken avsluttet, skal ikke bli en runtime-feil.
                if let Err(err) = result {
                    tracing::debug!(
                        task = %self.name,
                        error = %err,
                        "task stopped with error during shutdown"
                    );
                }
                return Ok(());
            }

            if run_duration >= self.stable_run_window && attempt > 0 {
                tracing::info!(
                    task = %self.name,
                    stable_for_ms = run_duration.as_millis() as u64,
                    "task recovered after restart loop"
                );
                attempt = 0;
                backoff = self.initial_backoff;
            }

            attempt += 1;

            match result {
                Ok(()) => {
                    tracing::warn!(
                        task = %self.name,
                        attempt,
                        run_duration_ms = run_duration.as_millis() as u64,
                        "task stopped unexpectedly"
                    );
                }
                Err(ref err) => {
                    tracing::warn!(
                        task = %self.name,
                        attempt,
                        run_duration_ms = run_duration.as_millis() as u64,
                        error = %err,
                        "task failed and will be restarted"
                    );
                }
            }

            if self
                .max_restart_attempts
                .is_some_and(|max_restart_attempts| attempt > max_restart_attempts)
            {
                let message = match result {
                    Ok(()) => format!("task {} stopped after exhausting restart budget", self.name),
                    Err(err) => {
                        format!(
                            "task {} failed after exhausting restart budget: {err}",
                            self.name
                        )
                    }
                };
                tracing::error!(task = %self.name, error = %message, "task exceeded restart budget");
                return Err(anyhow::anyhow!(message));
            }

            tracing::info!(
                task = %self.name,
                attempt,
                next_retry_ms = backoff.as_millis() as u64,
                "restarting critical task after backoff"
            );
            self.restarter.fetch_add(1, Ordering::Relaxed);
            if self.vent_med_backoff(backoff).await {
                return Ok(());
            }
            backoff = std::cmp::min(backoff.saturating_mul(2), self.max_backoff);
        }
    }

    /// Returnerer `true` når nedstenging avbrøt ventetiden.
    ///
    /// Backoffen kan være opptil 30 sekunder, mens Cloud Run gir 10. En
    /// ukansellerbar sleep her ville derfor gjort normal nedstenging til en
    /// hard kill.
    async fn vent_med_backoff(&self, backoff: Duration) -> bool {
        match &self.shutdown {
            Some(shutdown) => {
                tokio::select! {
                    _ = shutdown.cancelled() => true,
                    _ = tokio::time::sleep(backoff) => false,
                }
            }
            None => {
                tokio::time::sleep(backoff).await;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;
    use tokio_util::sync::CancellationToken;

    #[tokio::test(start_paused = true)]
    async fn nedstenging_avbryter_backoff_uten_aa_vente_ut_hele_forsinkelsen() {
        let shutdown = CancellationToken::new();
        let supervisor = TaskSupervisor::background("test").with_shutdown(shutdown.clone());
        let forsok = Arc::new(AtomicU32::new(0));

        let kjoring = {
            let forsok = forsok.clone();
            let shutdown = shutdown.clone();
            tokio::spawn(async move {
                supervisor
                    .run(|| {
                        let forsok = forsok.clone();
                        let shutdown = shutdown.clone();
                        async move {
                            if forsok.fetch_add(1, Ordering::SeqCst) == 0 {
                                // Første kjøring feiler og utløser backoff.
                                return Err(anyhow::anyhow!("uventet stopp"));
                            }
                            shutdown.cancelled().await;
                            Ok(())
                        }
                    })
                    .await
            })
        };

        // La første kjøring feile og backoffen starte, be så om nedstenging.
        for _ in 0..100 {
            if forsok.load(Ordering::SeqCst) == 1 {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            forsok.load(Ordering::SeqCst),
            1,
            "backoffen skal ha startet"
        );
        shutdown.cancel();

        let start = tokio::time::Instant::now();
        kjoring
            .await
            .expect("supervisor-tasken fullførte")
            .expect("nedstenging er ikke en feil");

        assert!(
            start.elapsed() < Duration::from_secs(10),
            "nedstenging skal ikke vente ut backoffen"
        );
        assert_eq!(
            forsok.load(Ordering::SeqCst),
            1,
            "tasken skal ikke restartes etter nedstenging"
        );
    }

    #[tokio::test]
    async fn helseflagget_er_nede_naar_tasken_har_falt_ut() {
        let helse = Helse::new();
        let supervisor = TaskSupervisor::critical("test", 0).with_helse(&helse);
        let oppe = helse.registrer_task("test");

        let resultat = supervisor
            .run(|| async { Err(anyhow::anyhow!("falt ut")) })
            .await;

        assert!(resultat.is_err(), "tomt budsjett returnerer Err");
        assert!(
            !oppe.load(Ordering::Relaxed),
            "readiness skal ikke påstå at en død task er oppe"
        );
        assert_eq!(supervisor.restarter(), 0, "budsjettet var tomt fra start");
    }

    #[tokio::test]
    async fn feil_under_nedstenging_rapporteres_ikke_som_runtime_feil() {
        let shutdown = CancellationToken::new();
        shutdown.cancel();
        let supervisor = TaskSupervisor::background("test").with_shutdown(shutdown);

        let resultat = supervisor
            .run(|| async { Err(anyhow::anyhow!("subscription avsluttet")) })
            .await;

        assert!(resultat.is_ok());
    }
}
