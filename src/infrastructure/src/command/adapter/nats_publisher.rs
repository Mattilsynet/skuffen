use crate::nats::client::NatsClient;
use crate::nats::jetstream_setup::{command_inbox_stream_config, ensure_stream};
use application::command::ports::command_dispatcher_port::CommandDispatcher;
use async_nats::HeaderMap;
use async_nats::jetstream::{self, message::PublishMessage};
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::Span;

#[derive(Clone)]
pub struct NatsCommandDispatcher {
    client: NatsClient,
}

impl NatsCommandDispatcher {
    pub fn new(client: NatsClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl CommandDispatcher for NatsCommandDispatcher {
    #[tracing::instrument(
        skip_all,
        name = "nats.publish.inbox",
        fields(
            command_id = %command.command_id,
            correlation_id = ?command.correlation_id,
            entity_type = tracing::field::Empty,
            subject = tracing::field::Empty
        )
    )]
    async fn dispatch(&self, command: &CommandEnvelope<Command>) -> Result<(), anyhow::Error> {
        let entity_type = match &command.payload {
            Command::OpprettSak(_) => "sak",
            Command::OpprettInngåendeJournalpost(_)
            | Command::OpprettUtgåendeJournalpost(_)
            | Command::OpprettInterntNotatJournalpost(_) => "journalpost",
            Command::AvsluttSak(_) => "sak",
        };

        let subject = format!("arkiv.command.inbox.{}.{}", entity_type, command.command_id);
        let payload = serde_json::to_vec(command)?;
        Span::current().record("entity_type", tracing::field::display(entity_type));
        Span::current().record("subject", tracing::field::display(subject.as_str()));

        let jetstream = jetstream::new(self.client.inner().clone());
        ensure_stream(
            &jetstream,
            command_inbox_stream_config(self.client.jetstream_replicas()),
        )
        .await?;
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
}
