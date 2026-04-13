use async_nats::jetstream::{self, AckKind};
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::{Span, error, info};

use crate::nats::client::NatsClient;
use crate::nats::jetstream_setup::{
    command_ready_stream_config, ensure_pull_consumer, ensure_stream, executor_consumer_config,
};
use crate::nats::supervisor::TaskSupervisor;
use application::command::ports::registrer_i_eksekveringssystem_port::RegistrerIEksekveringssystemUseCase;

pub struct KommandoEksekveringListener {
    client: NatsClient,
    use_case: Box<dyn RegistrerIEksekveringssystemUseCase>,
}

impl KommandoEksekveringListener {
    pub fn new(client: NatsClient, use_case: Box<dyn RegistrerIEksekveringssystemUseCase>) -> Self {
        Self { client, use_case }
    }

    #[tracing::instrument(skip_all, name = "nats.execution_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        let supervisor = TaskSupervisor::background("execution_listener");
        supervisor.run(|| self.run_once()).await
    }

    async fn run_once(&self) -> anyhow::Result<()> {
        let jetstream = jetstream::new(self.client.inner().clone());
        let replicas = self.client.jetstream_replicas();
        let stream = ensure_stream(&jetstream, command_ready_stream_config(replicas)).await?;
        let consumer =
            ensure_pull_consumer(&stream, "executor", executor_consumer_config(replicas)).await?;
        let mut messages = consumer.messages().await?;

        while let Some(message) = messages.next().await {
            let message = message.map_err(|err| anyhow::anyhow!("JetStream error: {err}"))?;
            self.process_message(message).await?;
        }

        Err(anyhow::anyhow!(
            "execution listener message stream ended unexpectedly"
        ))
    }

    #[tracing::instrument(
        skip_all,
        name = "command.register_execution",
        fields(
            command_id = tracing::field::Empty,
            correlation_id = tracing::field::Empty,
            traceparent = tracing::field::Empty
        )
    )]
    async fn process_message(&self, message: jetstream::Message) -> anyhow::Result<()> {
        crate::telemetry::record_traceparent_from_headers(message.headers.as_ref());

        let envelope: CommandEnvelope<Command> = match serde_json::from_slice(&message.payload) {
            Ok(cmd) => cmd,
            Err(err) => {
                error!("Failed to deserialize command: {err}");
                message.ack().await.map_err(|ack_err| {
                    anyhow::anyhow!("ack failed after deserialize error: {ack_err}")
                })?;
                return Ok(());
            }
        };

        Span::current().record("command_id", tracing::field::display(envelope.command_id));
        Span::current().record(
            "correlation_id",
            tracing::field::debug(envelope.correlation_id),
        );

        match self.use_case.handle(&envelope).await {
            Ok(()) => {
                message
                    .ack()
                    .await
                    .map_err(|err| anyhow::anyhow!("ack failed: {err}"))?;
            }
            Err(err) => {
                info!("Kunne ikke lagre kommando: {err}");
                message
                    .ack_with(AckKind::Nak(None))
                    .await
                    .map_err(|nak_err| anyhow::anyhow!("nak failed: {nak_err}"))?;
            }
        }

        Ok(())
    }
}
