use crate::command::status_event::to_public_status_event;
use crate::nats::client::NatsClient;
use application::command::ports::status_publisher_port::CommandStatusPublisher;
use async_nats::HeaderMap;
use async_nats::jetstream::{self, message::PublishMessage};
use async_trait::async_trait;
use domain::eksekvering::typer::CommandLifecycleEvent;
use tracing::Span;

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
    #[tracing::instrument(
        skip_all,
        name = "nats.publish.status",
        fields(
            command_id = %event.command_id,
            correlation_id = ?event.correlation_id,
            status = ?event.status,
            stage = event.stage.as_code(),
            stage_status = event.stage_status.as_code(),
            subject = tracing::field::Empty
        )
    )]
    async fn publish_status(&self, event: CommandLifecycleEvent) -> Result<(), anyhow::Error> {
        let subject = format!("arkiv.status.{}", event.command_id);
        let payload = serde_json::to_vec(&to_public_status_event(&event))?;
        let message_id = event.message_id();
        let jetstream = jetstream::new(self.client.inner().clone());
        Span::current().record("subject", tracing::field::display(subject.as_str()));
        jetstream
            .get_or_create_stream(jetstream::stream::Config {
                name: "arkiv_status".to_string(),
                subjects: vec!["arkiv.status.*".to_string()],
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
}
