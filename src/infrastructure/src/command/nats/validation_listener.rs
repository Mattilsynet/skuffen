use async_nats::jetstream::{self, AckKind};
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::{Instrument, Span, error};

use crate::command::wire_mapper::map_wire_envelope;
use crate::nats::client::NatsClient;
use crate::nats::jetstream_setup::{
    command_inbox_stream_config, command_ready_stream_config, ensure_pull_consumer, ensure_stream,
    validator_consumer_config,
};
use crate::nats::supervisor::TaskSupervisor;
use application::command::services::validate_command::ValidateCommandService;

pub struct CommandValidationListener {
    client: NatsClient,
    service: ValidateCommandService,
}

impl CommandValidationListener {
    pub fn new(client: NatsClient, service: ValidateCommandService) -> Self {
        Self { client, service }
    }

    #[tracing::instrument(skip_all, name = "nats.validation_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        let supervisor = TaskSupervisor::background("validation_listener");
        supervisor.run(|| self.run_once()).await
    }

    async fn run_once(&self) -> anyhow::Result<()> {
        let jetstream = jetstream::new(self.client.inner().clone());
        let replicas = self.client.jetstream_replicas();
        let stream = ensure_stream(&jetstream, command_inbox_stream_config(replicas)).await?;
        ensure_stream(&jetstream, command_ready_stream_config(replicas)).await?;
        let consumer =
            ensure_pull_consumer(&stream, "validator", validator_consumer_config(replicas)).await?;
        let mut messages = consumer.messages().await?;

        while let Some(message) = messages.next().await {
            let message = message.map_err(|err| anyhow::anyhow!("JetStream error: {err}"))?;
            self.process_message(message).await?;
        }

        Err(anyhow::anyhow!(
            "validation listener message stream ended unexpectedly"
        ))
    }

    /// Spanet får parent før det aktiveres, slik at valideringen havner i
    /// samme trace som mottaket.
    async fn process_message(&self, message: jetstream::Message) -> anyhow::Result<()> {
        let span = tracing::info_span!(
            "nats.validate",
            command_id = tracing::field::Empty,
            correlation_id = tracing::field::Empty,
        );
        crate::telemetry::set_parent_on_span_from_nats_headers(&span, message.headers.as_ref());
        self.handle_message(message).instrument(span).await
    }

    async fn handle_message(&self, message: jetstream::Message) -> anyhow::Result<()> {
        let (payload, acker) = message.split();

        let envelope: CommandEnvelope<Command> = match serde_json::from_slice(&payload.payload) {
            Ok(cmd) => cmd,
            Err(err) => {
                error!(
                    error_type = "deserialization",
                    payload_size = payload.payload.len(),
                    "kunne ikke deserialisere kommando: {err}"
                );
                ack_terminal(&acker).await?;
                return Ok(());
            }
        };

        Span::current().record("command_id", tracing::field::display(envelope.command_id));
        crate::telemetry::record_correlation_id(envelope.correlation_id);

        let application_envelope = map_wire_envelope(envelope);

        let outcome = match self.service.handle(application_envelope).await {
            Ok(outcome) => outcome,
            Err(err) => {
                error!(error = %err, "validering feilet, ber om ny levering");
                nak_retryable(&acker).await?;
                return Ok(());
            }
        };

        // Utfallet og årsaken logges av valideringstjenesten. Her avgjøres
        // bare om meldingen skal leveres på nytt.
        if outcome.is_retryable() {
            nak_retryable(&acker).await?;
        } else {
            ack_terminal(&acker).await?;
        }

        Ok(())
    }
}

async fn ack_terminal(acker: &jetstream::message::Acker) -> anyhow::Result<()> {
    acker
        .ack()
        .await
        .map_err(|err| anyhow::anyhow!("ack failed: {err}"))
}

async fn nak_retryable(acker: &jetstream::message::Acker) -> anyhow::Result<()> {
    acker
        .ack_with(AckKind::Nak(None))
        .await
        .map_err(|err| anyhow::anyhow!("nak failed: {err}"))
}
