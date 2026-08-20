use crate::command::status_event::{
    command_subject, operasjon_subject, to_public_command_status, to_public_operasjonstatus,
};
use crate::nats::client::NatsClient;
use crate::nats::jetstream_setup::{ensure_stream, status_stream_config};
use application::command::ports::status_publisher_port::StatusPublisher;
use async_nats::HeaderMap;
use async_nats::jetstream::{self, message::PublishMessage};
use async_trait::async_trait;
use domain::eksekvering::typer::{CommandStatus, Operasjonstatus};
use tracing::Span;

/// Én statusstrøm. Strømmen **er** loggen — en klient som vil ha historikken
/// lager en consumer med `DeliverPolicy::All`.
#[derive(Clone)]
pub struct NatsStatusPublisher {
    client: NatsClient,
}

impl NatsStatusPublisher {
    pub fn new(client: NatsClient) -> Self {
        Self { client }
    }

    async fn publiser(
        &self,
        subject: String,
        payload: Vec<u8>,
        message_id: String,
    ) -> Result<(), anyhow::Error> {
        let jetstream = jetstream::new(self.client.inner().clone());
        Span::current().record("subject", tracing::field::display(subject.as_str()));
        ensure_stream(
            &jetstream,
            status_stream_config(self.client.jetstream_replicas()),
        )
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

#[async_trait]
impl StatusPublisher for NatsStatusPublisher {
    #[tracing::instrument(
        skip_all,
        name = "nats.publish.command_status",
        fields(
            command_id = %status.command_id,
            correlation_id = ?status.correlation_id,
            hendelse = status.hendelse.as_code(),
            terminal = status.terminal,
            subject = tracing::field::Empty
        )
    )]
    async fn publiser_command_status(&self, status: CommandStatus) -> Result<(), anyhow::Error> {
        let subject = command_subject(status.command_id);
        let payload = serde_json::to_vec(&to_public_command_status(&status))?;
        // Dedupliseringsnøkkelen er id-er vi allerede har i databasen.
        self.publiser(subject, payload, status.message_id()).await
    }

    #[tracing::instrument(
        skip_all,
        name = "nats.publish.operasjonstatus",
        fields(
            command_id = %status.command_id,
            operasjon_id = %status.operasjon_id.0,
            operasjonstype = status.operasjonstype.as_code(),
            hendelse = status.hendelse.as_code(),
            attempt_no = status.attempt_no,
            subject = tracing::field::Empty
        )
    )]
    async fn publiser_operasjonstatus(&self, status: Operasjonstatus) -> Result<(), anyhow::Error> {
        let subject = operasjon_subject(status.command_id, status.operasjon_id.0);
        let payload = serde_json::to_vec(&to_public_operasjonstatus(&status))?;
        self.publiser(subject, payload, status.message_id()).await
    }
}
