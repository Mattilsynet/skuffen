use async_nats::jetstream;
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::{Instrument, Span, error};

use crate::command::nats::ack::{ack_terminal, leveringsnummer, logg_ny_levering, nak_med_backoff};
use crate::command::wire_mapper::map_wire_envelope;
use crate::http::helse::Helse;
use crate::nats::client::NatsClient;
use crate::nats::jetstream_setup::{
    command_ready_stream_config, ensure_pull_consumer, ensure_stream, executor_consumer_config,
};
use crate::nats::supervisor::{RESTARTBUDSJETT, TaskSupervisor, tasknavn};
use application::command::services::dekomponer_command::{
    DekomponerCommandService, DekomponeringsFeil,
};
use tokio_util::sync::CancellationToken;

/// Leser validerte kommandoer og dekomponerer dem til operasjoner.
///
/// Skjer én gang, her. Planen skrives i én transaksjon og er idempotent, så en
/// redelivery setter inn null rader.
pub struct DekomponeringListener {
    client: NatsClient,
    service: DekomponerCommandService,
    helse: Helse,
    shutdown: CancellationToken,
}

impl DekomponeringListener {
    pub fn new(
        client: NatsClient,
        service: DekomponerCommandService,
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

    #[tracing::instrument(skip_all, name = "nats.dekomponering_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        TaskSupervisor::critical(tasknavn::DEKOMPONERING_LISTENER, RESTARTBUDSJETT)
            .with_shutdown(self.shutdown.clone())
            .with_helse(&self.helse)
            .run(|| self.run_once())
            .await
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

    /// Spanet får parent før det aktiveres. Dette er siste steg som har
    /// meldingen i hånden — etter ack er trace-konteksten borte, og videre
    /// oppfølging skjer på `command_id` og `correlation_id`.
    async fn process_message(&self, message: jetstream::Message) -> anyhow::Result<()> {
        let span = tracing::info_span!(
            "nats.dekomponer",
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

        match self.service.handle(application_envelope).await {
            Ok(()) => ack_terminal(&acker).await?,
            // Tjenesten har allerede publisert `Feilet`. Uten `max_deliver` og
            // uten DLQ er denne acken eneste vei ut for en melding som aldri
            // kan lykkes (SKU-0017 R8).
            Err(DekomponeringsFeil::Permanent { kode, .. }) => {
                error!(
                    kode,
                    delivered, "dekomponeringen kan ikke lykkes, kommandoen avsluttes"
                );
                ack_terminal(&acker).await?;
            }
            Err(DekomponeringsFeil::Transient(err)) => {
                logg_ny_levering(delivered, Some(command_id), "dekomponeringen feilet");
                error!(error = %format!("{err:#}"), "kunne ikke dekomponere kommandoen");
                nak_med_backoff(&acker, delivered).await?;
            }
        }

        Ok(())
    }
}
