//! Leser originalkommandoen fra inbox-strømmen. Publiserer ingenting.

use anyhow::Context;
use application::admin::ports::admin_retry_port::OriginalCommandKilde;
use application::command::{Command, CommandEnvelope};
use async_nats::jetstream::{self, stream::LastRawMessageErrorKind};
use async_trait::async_trait;
use domain::eksekvering::typer::CommandTypeCode;
use lib_schemas::skuffen::command::commands::{
    Command as WireCommand, CommandEnvelope as WireCommandEnvelope,
};
use uuid::Uuid;

use crate::command::wire_mapper::map_wire_envelope;
use crate::command::wire_routing_token::{
    CommandStreamStage, command_subject_for_token, routing_token_from_command_type,
};
use crate::nats::client::NatsClient;
use crate::nats::jetstream_setup::command_inbox_stream_config;

pub struct NatsOriginalCommandKilde {
    jetstream: jetstream::Context,
    stream_name: String,
}

impl NatsOriginalCommandKilde {
    pub fn new(client: NatsClient) -> Self {
        let stream_name = command_inbox_stream_config(client.jetstream_replicas()).name;
        Self {
            jetstream: jetstream::new(client.inner().clone()),
            stream_name,
        }
    }
}

#[async_trait]
impl OriginalCommandKilde for NatsOriginalCommandKilde {
    async fn hent(
        &self,
        command_id: Uuid,
        command_type: CommandTypeCode,
    ) -> anyhow::Result<Option<CommandEnvelope<Command>>> {
        let stream = self
            .jetstream
            .get_stream(self.stream_name.clone())
            .await
            .context("failed to load command inbox stream")?;
        let subject = command_subject_for_token(
            CommandStreamStage::Inbox,
            routing_token_from_command_type(command_type),
            command_id,
        );

        let melding = match stream.get_last_raw_message_by_subject(&subject).await {
            Ok(melding) => melding,
            Err(err) if matches!(err.kind(), LastRawMessageErrorKind::NoMessageFound) => {
                return Ok(None);
            }
            Err(err) => {
                return Err(anyhow::Error::new(err).context("failed to read original command"));
            }
        };

        let envelope: WireCommandEnvelope<WireCommand> =
            serde_json::from_slice(&melding.payload)
                .context("original command could not be deserialized")?;
        anyhow::ensure!(
            envelope.command_id == command_id,
            "original command on subject has a different command_id"
        );
        Ok(Some(map_wire_envelope(envelope)))
    }
}
