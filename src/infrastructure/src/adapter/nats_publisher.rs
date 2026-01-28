use crate::nats::client::NatsClient;
use application::ports::command_dispatcher_port::CommandDispatcher;
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{CommandEnvelope, Kommando};

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
    async fn dispatch(&self, command: &CommandEnvelope<Kommando>) -> Result<(), anyhow::Error> {
        let entity_type = match &command.payload {
            Kommando::OpprettSak(_) => "sak",
            Kommando::OpprettInngåendeJournalpost(_)
            | Kommando::OpprettUtgåendeJournalpost(_)
            | Kommando::OpprettInterntNotatJournalpost(_) => "journalpost",
            Kommando::AvsluttSak(_) => "sak",
        };

        let subject = format!("skuffen.stream.command.{}", entity_type);
        let payload = serde_json::to_vec(command)?;

        self.client.inner().publish(subject, payload.into()).await?;
        Ok(())
    }
}
