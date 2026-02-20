use chrono::{DateTime, Utc};
use tokio::time::{sleep, Duration};

use crate::command::ports::eksekvering_state_port::{
    EksekveringKommando, EksekveringStateRepository, EksekveringStatus,
};
use crate::command::services::eksekver_kommando::{EksekverKommandoService, ExecutionOutcome};

pub struct EksekveringWorker {
    state_repo: Box<dyn EksekveringStateRepository>,
    executor: EksekverKommandoService,
    worker_id: String,
    poll_interval: Duration,
    batch_size: i64,
}

impl EksekveringWorker {
    pub fn new(
        state_repo: Box<dyn EksekveringStateRepository>,
        executor: EksekverKommandoService,
        worker_id: String,
        poll_interval: Duration,
        batch_size: i64,
    ) -> Self {
        Self {
            state_repo,
            executor,
            worker_id,
            poll_interval,
            batch_size,
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        loop {
            let commands = self
                .state_repo
                .hent_klare_kommandoer(self.batch_size, &self.worker_id)
                .await?;

            if commands.is_empty() {
                sleep(self.poll_interval).await;
                continue;
            }

            for command in commands {
                self.execute_one(command).await?;
            }
        }
    }

    async fn execute_one(&self, command: EksekveringKommando) -> anyhow::Result<()> {
        let outcome = self.executor.handle(command.envelope).await?;
        match outcome {
            ExecutionOutcome::Ok => {
                self.state_repo
                    .oppdater_eksekvering(command.command_id, EksekveringStatus::Ok, None, None)
                    .await?;
            }
            ExecutionOutcome::Error => {
                self.state_repo
                    .oppdater_eksekvering(command.command_id, EksekveringStatus::Error, None, None)
                    .await?;
            }
            ExecutionOutcome::Blocked => {
                let next_retry = self.neste_retry_at(command.attempts);
                self.state_repo
                    .oppdater_eksekvering(
                        command.command_id,
                        EksekveringStatus::Blocked,
                        None,
                        Some(next_retry),
                    )
                    .await?;
            }
            ExecutionOutcome::Retrying => {
                let next_retry = self.neste_retry_at(command.attempts);
                self.state_repo
                    .oppdater_eksekvering(
                        command.command_id,
                        EksekveringStatus::Retrying,
                        None,
                        Some(next_retry),
                    )
                    .await?;
            }
        }
        Ok(())
    }

    fn neste_retry_at(&self, attempt: i32) -> DateTime<Utc> {
        let attempt = if attempt < 0 { 0 } else { attempt as u32 };
        crate::command::services::eksekvering_backoff::neste_backoff(attempt)
    }
}
