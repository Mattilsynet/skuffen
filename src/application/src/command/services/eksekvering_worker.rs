use chrono::{DateTime, Utc};
use tokio::time::{sleep, Duration};

use crate::command::ports::command_execution_port::CommandExecutionRepository;
use crate::command::services::eksekver_kommando::{EksekverKommandoService, ExecutionOutcome};

pub struct EksekveringWorker {
    execution_repo: Box<dyn CommandExecutionRepository>,
    executor: EksekverKommandoService,
    worker_id: String,
    poll_interval: Duration,
}

impl EksekveringWorker {
    pub fn new(
        execution_repo: Box<dyn CommandExecutionRepository>,
        executor: EksekverKommandoService,
        worker_id: String,
        poll_interval: Duration,
    ) -> Self {
        Self {
            execution_repo,
            executor,
            worker_id,
            poll_interval,
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        if !self
            .execution_repo
            .try_acquire_executor_lock(&self.worker_id)
            .await?
        {
            return Ok(());
        }

        let _ = self.execution_repo.reset_kjorer_til_klar().await?;

        loop {
            let Some(command) = self.execution_repo.hent_neste_kjorbare().await? else {
                sleep(self.poll_interval).await;
                continue;
            };

            self.execute_one(command.command_id, command.envelope)
                .await?;
        }
    }

    async fn execute_one(
        &self,
        command_id: uuid::Uuid,
        envelope: lib_schemas::skuffen::command::commands::CommandEnvelope<
            lib_schemas::skuffen::command::commands::Command,
        >,
    ) -> anyhow::Result<()> {
        let attempt_no = self.execution_repo.marker_kjorer(command_id).await?;
        self.execution_repo
            .registrer_forsok(command_id, attempt_no, &self.worker_id)
            .await?;

        let outcome = self.executor.handle(envelope, attempt_no as u32).await?;
        match outcome {
            ExecutionOutcome::Ok => {
                self.execution_repo
                    .marker_ok(command_id, attempt_no)
                    .await?;
            }
            ExecutionOutcome::Feil { last_error } => {
                self.execution_repo
                    .marker_feil(
                        command_id,
                        attempt_no,
                        last_error.as_deref().unwrap_or("ukjent execution-feil"),
                    )
                    .await?;
            }
            ExecutionOutcome::BlokkertVenter { last_error } => {
                self.execution_repo
                    .marker_blokkert_venter(
                        command_id,
                        attempt_no,
                        last_error
                            .as_deref()
                            .unwrap_or("kommando venter på prerequisite"),
                    )
                    .await?;
            }
            ExecutionOutcome::Retrying { last_error } => {
                let next_retry = self.neste_retry_at(attempt_no - 1);
                self.execution_repo
                    .marker_retry_venter(
                        command_id,
                        attempt_no,
                        last_error
                            .as_deref()
                            .unwrap_or("recoverable execution-feil"),
                        next_retry,
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
