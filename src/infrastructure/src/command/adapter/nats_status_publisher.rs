use crate::nats::client::NatsClient;
use application::command::ports::status_publisher_port::CommandStatusPublisher;
use async_nats::jetstream::{self, PublishMessage};
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::CommandStatusEvent;

#[derive(Clone)]
pub struct NatsCommandStatusPublisher {
    client: NatsClient,
}

impl NatsCommandStatusPublisher {
    pub fn new(client: NatsClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl CommandStatusPublisher for NatsCommandStatusPublisher {
    async fn publish_status(&self, event: CommandStatusEvent) -> Result<(), anyhow::Error> {
        let subject = "arkiv.status";
        let payload = serde_json::to_vec(&event)?;
        let jetstream = jetstream::new(self.client.inner().clone());
        jetstream
            .send_publish(
                subject,
                PublishMessage::build()
                    .payload(payload.into())
                    .message_id(event.command_id.to_string()),
            )
            .await?
            .await?;
        Ok(())
    }
}
