use crate::nats::client::NatsClient;
use application::command::ports::eksekvering_port::EksekveringKvitteringPublisher;
use async_nats::HeaderMap;
use async_nats::jetstream::{self, message::PublishMessage};
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::Span;

#[derive(Clone)]
pub struct NatsDonePublisher {
    client: NatsClient,
}

impl NatsDonePublisher {
    pub fn new(client: NatsClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl EksekveringKvitteringPublisher for NatsDonePublisher {
    #[tracing::instrument(
        skip_all,
        name = "nats.publish.done",
        fields(
            command_id = %command.command_id,
            correlation_id = ?command.correlation_id,
            subject = %subject
        )
    )]
    async fn publiser_done(
        &self,
        subject: &str,
        command: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error> {
        let payload = serde_json::to_vec(command)?;
        let jetstream = jetstream::new(self.client.inner().clone());
        Span::current().record("subject", tracing::field::display(subject));
        jetstream
            .get_or_create_stream(jetstream::stream::Config {
                name: "arkiv_command_done".to_string(),
                subjects: vec!["arkiv.command.done.>".to_string()],
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
            .send_publish(
                subject.to_string(),
                message.message_id(command.command_id.to_string()),
            )
            .await?
            .await?;
        Ok(())
    }
}
