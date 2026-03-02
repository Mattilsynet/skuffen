use crate::nats::client::NatsClient;
use application::command::ports::validated_command_dispatcher_port::ValidatedCommandDispatcher;
use async_nats::jetstream::{self, message::PublishMessage};
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::Instrument;
use async_nats::HeaderMap;

#[derive(Clone)]
pub struct NatsValidatedCommandDispatcher {
    client: NatsClient,
}

impl NatsValidatedCommandDispatcher {
    pub fn new(client: NatsClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ValidatedCommandDispatcher for NatsValidatedCommandDispatcher {
    async fn dispatch_validated(
        &self,
        command: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error> {
        let entity_type = match &command.payload {
            Command::OpprettSak(_) | Command::AvsluttSak(_) => "sak",
            Command::OpprettInngåendeJournalpost(_)
            | Command::OpprettUtgåendeJournalpost(_)
            | Command::OpprettInterntNotatJournalpost(_) => "journalpost",
        };

        let subject = format!("arkiv.command.ready.{}.{}", entity_type, command.command_id);
        let payload = serde_json::to_vec(command)?;

        let jetstream = jetstream::new(self.client.inner().clone());
        let span = tracing::info_span!(
            "nats.publish.ready",
            command_id = %command.command_id,
            correlation_id = ?command.correlation_id,
            entity_type,
            subject = %subject
        );
        async move {
            let mut message = PublishMessage::build().payload(payload.into());
            if let Some(trace_parent) = crate::telemetry::current_trace_parent() {
                let mut headers = HeaderMap::new();
                headers.insert("traceparent", trace_parent);
                message = message.headers(headers);
            }
            jetstream
                .send_publish(subject, message.message_id(command.command_id.to_string()))
                .await?
                .await?;
            Ok(())
        }
        .instrument(span)
        .await
    }
}
