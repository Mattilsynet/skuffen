use async_nats::jetstream::{self, consumer};
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::{Span, error, info};

use crate::nats::client::NatsClient;
use application::command::services::validate_command::{ValidateCommandService, ValidationOutcome};

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
        let jetstream = jetstream::new(self.client.inner().clone());
        let stream = match jetstream
            .get_or_create_stream(jetstream::stream::Config {
                name: "arkiv_command_inbox".to_string(),
                subjects: vec!["arkiv.command.inbox.>".to_string()],
                max_age: std::time::Duration::from_secs(60 * 60 * 24 * 180),
                ..Default::default()
            })
            .await
        {
            Ok(stream) => stream,
            Err(err) => return Err(anyhow::anyhow!("JetStream stream error: {err}")),
        };

        let _ = match jetstream
            .get_or_create_stream(jetstream::stream::Config {
                name: "arkiv_command_ready".to_string(),
                subjects: vec!["arkiv.command.ready.>".to_string()],
                max_age: std::time::Duration::from_secs(60 * 60 * 24 * 180),
                ..Default::default()
            })
            .await
        {
            Ok(stream) => stream,
            Err(err) => return Err(anyhow::anyhow!("JetStream ready stream error: {err}")),
        };

        let consumer = match stream
            .get_or_create_consumer(
                "validator",
                consumer::pull::Config {
                    durable_name: Some("validator".to_string()),
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
        name = "command.validate",
        fields(
            command_id = tracing::field::Empty,
            correlation_id = tracing::field::Empty,
            traceparent = tracing::field::Empty
        )
    )]
    async fn process_message(&self, message: jetstream::Message) {
        let (payload, acker) = message.split();
        crate::telemetry::record_traceparent_from_headers(payload.headers.as_ref());

        let envelope: CommandEnvelope<Command> = match serde_json::from_slice(&payload.payload) {
            Ok(cmd) => cmd,
            Err(err) => {
                error!("Failed to deserialize command: {err}");
                if let Err(err) = acker.ack().await {
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

        let outcome = match self.service.handle(envelope).await {
            Ok(outcome) => outcome,
            Err(err) => {
                error!("Validator failed: {err}");
                if let Err(err) = acker.ack().await {
                    error!("Ack failed: {err}");
                }
                return;
            }
        };

        match outcome {
            ValidationOutcome::Ok => {
                if let Err(err) = acker.ack().await {
                    error!("Ack failed: {err}");
                }
            }
            ValidationOutcome::Recoverable { message: reason } => {
                info!("Command recoverable, retrying later: {reason}");
                if let Err(err) = acker.ack().await {
                    error!("Ack failed: {err}");
                }
            }
            ValidationOutcome::Blocked { message: reason } => {
                info!("Command blocked, retrying later: {reason}");
                if let Err(err) = acker.ack().await {
                    error!("Ack failed: {err}");
                }
            }
            ValidationOutcome::Irrecoverable { message: reason } => {
                info!("Command irrecoverable: {reason}");
                if let Err(err) = acker.ack().await {
                    error!("Ack failed: {err}");
                }
            }
        }
    }
}
