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

    /// Gjør supervisoren nedstengingsbevisst.
    ///
    /// Uten dette ville en task som avslutter pent ved SIGTERM blitt restartet
    /// umiddelbart — supervisoren kan ikke se forskjell på «stoppet fordi den
    /// skal» og «stoppet uventet».
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
                return result;
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
            tokio::time::sleep(backoff).await;
            backoff = std::cmp::min(backoff.saturating_mul(2), self.max_backoff);
        }
    }
}
