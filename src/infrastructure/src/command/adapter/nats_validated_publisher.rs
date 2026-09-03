use crate::command::wire_mapper::map_application_envelope_to_wire;
use crate::command::wire_routing_token::{CommandStreamStage, command_subject};
use crate::nats::client::NatsClient;
use application::command::ports::validated_command_dispatcher_port::ValidatedCommandDispatcher;
use application::command::{Command, CommandEnvelope};
use async_nats::jetstream::{self, message::PublishMessage};
use async_trait::async_trait;
use tracing::Span;

/// Ready-strømmen opprettes i `prepare_runtime`, ikke her.
#[derive(Clone)]
pub struct NatsValidatedCommandDispatcher {
    jetstream: jetstream::Context,
}

impl NatsValidatedCommandDispatcher {
    pub fn new(client: NatsClient) -> Self {
        Self {
            jetstream: jetstream::new(client.inner().clone()),
        }
    }
}

#[async_trait]
impl ValidatedCommandDispatcher for NatsValidatedCommandDispatcher {
    #[tracing::instrument(
        skip_all,
        name = "nats.publish.ready",
        fields(
            command_id = %command.command_id,
            correlation_id = tracing::field::Empty,
            subject = tracing::field::Empty
        )
    )]
    async fn dispatch_validated(
        &self,
        command: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error> {
        crate::telemetry::record_correlation_id(command.correlation_id);
        let wire_envelope = map_application_envelope_to_wire(command)?;
        let subject = command_subject(
            CommandStreamStage::Ready,
            &wire_envelope.payload,
            command.command_id,
        );
        let payload = serde_json::to_vec(&wire_envelope)?;
        Span::current().record("subject", tracing::field::display(subject.as_str()));

        let mut message = PublishMessage::build().payload(payload.into());
        if let Some(headers) = crate::telemetry::trace_headers() {
            message = message.headers(headers);
        }
        self.jetstream
            .send_publish(subject, message.message_id(command.command_id.to_string()))
            .await?
            .await?;
        Ok(())
    }
}
