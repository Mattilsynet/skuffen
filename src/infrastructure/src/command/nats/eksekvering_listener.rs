use async_nats::jetstream::{self, AckKind, consumer};
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::{Span, error, info};

use crate::nats::client::NatsClient;
use application::command::ports::registrer_eksekvering_port::RegistrerEksekveringUseCase;

pub struct KommandoEksekveringListener {
    client: NatsClient,
    use_case: Box<dyn RegistrerEksekveringUseCase>,
}

impl KommandoEksekveringListener {
    pub fn new(client: NatsClient, use_case: Box<dyn RegistrerEksekveringUseCase>) -> Self {
        Self { client, use_case }
    }

    #[tracing::instrument(skip_all, name = "nats.execution_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        let jetstream = jetstream::new(self.client.inner().clone());
        let stream = match jetstream
            .get_or_create_stream(jetstream::stream::Config {
                name: "arkiv_command_ready".to_string(),
                subjects: vec!["arkiv.command.ready.>".to_string()],
                max_age: std::time::Duration::from_secs(60 * 60 * 24 * 180),
                ..Default::default()
            })
            .await
        {
            Ok(stream) => stream,
            Err(err) => return Err(anyhow::anyhow!("JetStream stream error: {err}")),
        };

        let consumer = match stream
            .get_or_create_consumer(
                "executor",
                consumer::pull::Config {
                    durable_name: Some("executor".to_string()),
                    ack_policy: consumer::AckPolicy::Explicit,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(consumer) => consumer,
            Err(err) => return Err(anyhow::anyhow!("JetStream consumer create error: {err}")),
        };

        let mut messages = match consumer.messages().await {
            Ok(messages) => messages,
            Err(err) => return Err(anyhow::anyhow!("JetStream consumer error: {err}")),
        };

        while let Some(message) = messages.next().await {
            let message = match message {
                Ok(msg) => msg,
                Err(err) => {
                    error!("JetStream error: {err}");
                    continue;
                }
            };
            self.process_message(message).await;
        }

        Ok(())
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
    async fn process_message(&self, message: jetstream::Message) {
        crate::telemetry::record_traceparent_from_headers(message.headers.as_ref());

        let envelope: CommandEnvelope<Command> = match serde_json::from_slice(&message.payload) {
            Ok(cmd) => cmd,
            Err(err) => {
                error!("Failed to deserialize command: {err}");
                if let Err(err) = message.ack().await {
                    error!("Ack failed: {err}");
                }
                return;
            }
        };

        Span::current().record("command_id", tracing::field::display(envelope.command_id));
        Span::current().record(
            "correlation_id",
            tracing::field::debug(envelope.correlation_id),
        );

        match self.use_case.handle(&envelope).await {
            Ok(()) => {
                if let Err(err) = message.ack().await {
                    error!("Ack failed: {err}");
                }
            }
            Err(err) => {
                info!("Kunne ikke lagre kommando: {err}");
                if let Err(err) = message.ack_with(AckKind::Nak(None)).await {
                    error!("NAK failed: {err}");
                }
            }
        }
    }
}
