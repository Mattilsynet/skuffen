use crate::nats::client::NatsClient;
use application::command::ports::command_dispatcher_port::CommandDispatcher;
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

        let subject = format!("skuffen.stream.command.{}", entity_type);
        let payload = serde_json::to_vec(command)?;

        self.client.inner().publish(subject, payload.into()).await?;
        Ok(())
    }
}
