use crate::nats::client::NatsClient;
use application::command::ports::validated_command_dispatcher_port::ValidatedCommandDispatcher;
use async_nats::jetstream::{self, PublishMessage};
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

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

        let subject = format!(
            "arkiv.command.ready.{}.{}",
            entity_type, command.command_id
        );
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
