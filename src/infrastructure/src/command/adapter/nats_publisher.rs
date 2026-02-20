use crate::nats::client::NatsClient;
use application::command::ports::command_dispatcher_port::CommandDispatcher;
use async_nats::jetstream::{self, message::PublishMessage};
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

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

        let jetstream = jetstream::new(self.client.inner().clone());
        jetstream
            .send_publish(
                subject,
                PublishMessage::build()
                    .payload(payload.into())
                    .message_id(command.command_id.to_string()),
            )
            .await?
            .await?;
        Ok(())
    }
}
