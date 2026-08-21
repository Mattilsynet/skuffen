use async_nats::jetstream::{self, AckKind};
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::{Span, error, warn};

use crate::command::wire_mapper::map_wire_envelope;
use crate::nats::client::NatsClient;
use crate::nats::jetstream_setup::{
    command_ready_stream_config, ensure_pull_consumer, ensure_stream, executor_consumer_config,
};
use crate::nats::supervisor::TaskSupervisor;
use application::command::services::dekomponer_command::DekomponerCommandService;

/// Leser validerte kommandoer og dekomponerer dem til operasjoner.
///
/// Skjer én gang, her. Planen skrives i én transaksjon og er idempotent, så en
/// redelivery setter inn null rader.
pub struct DekomponeringListener {
    client: NatsClient,
    service: DekomponerCommandService,
}

impl DekomponeringListener {
    pub fn new(client: NatsClient, service: DekomponerCommandService) -> Self {
        Self { client, service }
    }

    #[tracing::instrument(skip_all, name = "nats.dekomponering_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        let supervisor = TaskSupervisor::background("dekomponering_listener");
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
            "dekomponering listener message stream ended unexpectedly"
        ))
    }

    #[tracing::instrument(
        skip_all,
        name = "command.dekomponer",
        fields(
            command_id = tracing::field::Empty,
            correlation_id = tracing::field::Empty,
        )
    )]
    async fn process_message(&self, message: jetstream::Message) -> anyhow::Result<()> {
        crate::telemetry::set_parent_from_nats_headers(message.headers.as_ref());

        let envelope: CommandEnvelope<Command> = match serde_json::from_slice(&message.payload) {
            Ok(cmd) => cmd,
            Err(err) => {
                error!(
                    error_type = "deserialization",
                    payload_size = message.payload.len(),
                    "Failed to deserialize command: {err}"
                );
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

        let application_envelope = map_wire_envelope(envelope);

        match self.service.handle(application_envelope).await {
            Ok(()) => {
                message
                    .ack()
                    .await
                    .map_err(|err| anyhow::anyhow!("ack failed: {err}"))?;
            }
            Err(err) => {
                warn!("Kunne ikke dekomponere kommandoen: {err:#}");
                message
                    .ack_with(AckKind::Nak(Some(std::time::Duration::from_secs(30))))
                    .await
                    .map_err(|nak_err| anyhow::anyhow!("nak failed: {nak_err}"))?;
            }
        }

        Ok(())
    }
}
