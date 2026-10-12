use crate::command::wire_mapper::map_application_envelope_to_wire;
use crate::command::wire_routing_token::{CommandStreamStage, command_subject};
use crate::nats::client::NatsClient;
use application::command::ports::validated_command_dispatcher_port::ValidatedCommandDispatcher;
use application::command::{Command, CommandEnvelope};
use async_nats::jetstream::{self, message::PublishMessage};
use async_trait::async_trait;
use tracing::Span;

/// JetStream-dedup nøkles på meldings-id innen stream-ens dedup-vindu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Meldingsid {
    /// Ordinær validering: en redelivery av samme kommando dedupliseres.
    CommandId,
    /// Admin-initiert revalidering skal ikke slukes av en tidligere
    /// ready-melding for samme kommando. Dekomponeringen er idempotent, så
    /// en ekstra levering er en no-op.
    UnikPerForsok,
}

/// Ready-strømmen opprettes i `prepare_runtime`, ikke her.
#[derive(Clone)]
pub struct NatsValidatedCommandDispatcher {
    jetstream: jetstream::Context,
    meldingsid: Meldingsid,
}

impl NatsValidatedCommandDispatcher {
    pub fn new(client: NatsClient) -> Self {
        Self {
            jetstream: jetstream::new(client.inner().clone()),
            meldingsid: Meldingsid::CommandId,
        }
    }

    pub fn for_admin_revalidering(client: NatsClient) -> Self {
        Self {
            meldingsid: Meldingsid::UnikPerForsok,
            ..Self::new(client)
        }
    }
}

fn meldingsid(policy: Meldingsid, command_id: uuid::Uuid) -> String {
    match policy {
        Meldingsid::CommandId => command_id.to_string(),
        Meldingsid::UnikPerForsok => format!("{command_id}.admin.{}", uuid::Uuid::now_v7()),
    }
}

#[async_trait]
impl ValidatedCommandDispatcher for NatsValidatedCommandDispatcher {
    #[tracing::instrument(
        skip_all,
        name = "nats.publish.ready",
        fields(
            command_id = %command.command_id,
            correlation_id = tracing::field::Empty,
            subject = tracing::field::Empty
        )
    )]
    async fn dispatch_validated(
        &self,
        command: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error> {
        crate::telemetry::record_correlation_id(command.correlation_id);
        let wire_envelope = map_application_envelope_to_wire(command)?;
        let subject = command_subject(
            CommandStreamStage::Ready,
            &wire_envelope.payload,
            command.command_id,
        );
        let payload = serde_json::to_vec(&wire_envelope)?;
        Span::current().record("subject", tracing::field::display(subject.as_str()));

        let mut message = PublishMessage::build().payload(payload.into());
        if let Some(headers) = crate::telemetry::trace_headers() {
            message = message.headers(headers);
        }
        self.jetstream
            .send_publish(
                subject,
                message.message_id(meldingsid(self.meldingsid, command.command_id)),
            )
            .await?
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinaer_validering_dedupliseres_paa_command_id() {
        let command_id = uuid::Uuid::new_v4();
        assert_eq!(
            meldingsid(Meldingsid::CommandId, command_id),
            command_id.to_string()
        );
    }

    #[test]
    fn admin_revalidering_faar_ny_meldingsid_hver_gang() {
        let command_id = uuid::Uuid::new_v4();
        let forste = meldingsid(Meldingsid::UnikPerForsok, command_id);
        let andre = meldingsid(Meldingsid::UnikPerForsok, command_id);
        assert_ne!(forste, andre);
        assert_ne!(forste, command_id.to_string());
        assert!(forste.starts_with(&command_id.to_string()));
    }
}
