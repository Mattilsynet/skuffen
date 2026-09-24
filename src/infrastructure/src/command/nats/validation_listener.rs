use async_nats::jetstream;
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::{Instrument, Span, error};

use crate::command::nats::ack::{ack_terminal, leveringsnummer, logg_ny_levering, nak_med_backoff};
use crate::command::wire_mapper::map_wire_envelope;
use crate::http::helse::Helse;
use crate::nats::client::NatsClient;
use crate::nats::jetstream_setup::{
    command_inbox_stream_config, command_ready_stream_config, ensure_pull_consumer, ensure_stream,
    validator_consumer_config,
};
use crate::nats::supervisor::{RESTARTBUDSJETT, TaskSupervisor, tasknavn};
use application::command::services::validate_command::ValidateCommandService;
use tokio_util::sync::CancellationToken;

pub struct CommandValidationListener {
    client: NatsClient,
    service: ValidateCommandService,
    helse: Helse,
    shutdown: CancellationToken,
}

impl CommandValidationListener {
    pub fn new(
        client: NatsClient,
        service: ValidateCommandService,
        helse: Helse,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            client,
            service,
            helse,
            shutdown,
        }
    }

    #[tracing::instrument(skip_all, name = "nats.validation_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        TaskSupervisor::critical(tasknavn::VALIDATION_LISTENER, RESTARTBUDSJETT)
            .with_shutdown(self.shutdown.clone())
            .with_helse(&self.helse)
            .run(|| self.run_once())
            .await
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
        let delivered = leveringsnummer(&message);
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

        let command_id = envelope.command_id;
        let application_envelope = map_wire_envelope(envelope);

        let outcome = match self.service.handle(application_envelope).await {
            Ok(outcome) => outcome,
            Err(err) => {
                logg_ny_levering(delivered, Some(command_id), "valideringen feilet");
                error!(error = %err, "validering feilet, ber om ny levering");
                nak_med_backoff(&acker, delivered).await?;
                return Ok(());
            }
        };

        // Utfallet og årsaken logges av valideringstjenesten. Her avgjøres
        // bare om meldingen skal leveres på nytt.
        if outcome.is_retryable() {
            logg_ny_levering(delivered, Some(command_id), "valideringen er ikke avklart");
            nak_med_backoff(&acker, delivered).await?;
        } else {
            ack_terminal(&acker).await?;
        }

        Ok(())
    }
}
