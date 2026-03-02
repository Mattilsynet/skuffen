use crate::nats::client::NatsClient;
use application::command::ports::status_publisher_port::CommandStatusPublisher;
use async_nats::jetstream::{self, message::PublishMessage};
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::CommandStatusEvent;
use tracing::Instrument;
use async_nats::HeaderMap;

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
        let message_id = format!("{}:{}", event.command_id, uuid::Uuid::now_v7());
        let jetstream = jetstream::new(self.client.inner().clone());
        let span = tracing::info_span!(
            "nats.publish.status",
            command_id = %event.command_id,
            status = ?event.status
        );
        async move {
            jetstream
                .get_or_create_stream(jetstream::stream::Config {
                    name: "arkiv_status".to_string(),
                    subjects: vec!["arkiv.status".to_string()],
                    max_age: std::time::Duration::from_secs(60 * 60 * 24 * 180),
                    ..Default::default()
                })
                .await?;
            let mut message = PublishMessage::build().payload(payload.into());
            if let Some(trace_parent) = crate::telemetry::current_trace_parent() {
                let mut headers = HeaderMap::new();
                headers.insert("traceparent", trace_parent);
                message = message.headers(headers);
            }
            jetstream
                .send_publish(subject, message.message_id(message_id))
                .await?
                .await?;
            Ok(())
        }
        .instrument(span)
        .await
    }
}
