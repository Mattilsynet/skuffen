use std::future::Future;
use std::time::{Duration, Instant};

pub struct TaskSupervisor {
    name: String,
    initial_backoff: Duration,
    max_backoff: Duration,
    stable_run_window: Duration,
    max_restart_attempts: Option<u32>,
    shutdown: Option<tokio_util::sync::CancellationToken>,
}

impl TaskSupervisor {
    fn er_nedstengt(&self) -> bool {
        self.shutdown
            .as_ref()
            .is_some_and(tokio_util::sync::CancellationToken::is_cancelled)
    }

    pub fn critical(name: impl Into<String>, max_restart_attempts: u32) -> Self {
        Self {
            name: name.into(),
            initial_backoff: Duration::from_secs(1),
            max_backoff: Duration::from_secs(30),
            stable_run_window: Duration::from_secs(30),
            max_restart_attempts: Some(max_restart_attempts),
            shutdown: None,
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
        }
    }

    /// Skiller «stoppet fordi den skal» fra «stoppet uventet», slik at en task
    /// som avslutter ved SIGTERM ikke restartes.
    pub fn with_shutdown(mut self, shutdown: tokio_util::sync::CancellationToken) -> Self {
        self.shutdown = Some(shutdown);
        self
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
                return Ok(());
            }

            let started_at = Instant::now();
            let result = run_once().await;
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
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
