use async_nats::jetstream::{self, AckKind, consumer};
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::{error, info};

use crate::nats::client::NatsClient;
use application::command::ports::eksekvering_state_port::EksekveringStateRepository;

pub struct KommandoEksekveringListener {
    client: NatsClient,
    state_repo: Box<dyn EksekveringStateRepository>,
}

impl KommandoEksekveringListener {
    pub fn new(client: NatsClient, state_repo: Box<dyn EksekveringStateRepository>) -> Self {
        Self { client, state_repo }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let jetstream = jetstream::new(self.client.inner().clone());
        let stream = match jetstream
            .get_or_create_stream(jetstream::stream::Config {
                name: "arkiv_command_ready".to_string(),
                subjects: vec!["arkiv.command.ready.>".to_string()],
                max_age: std::time::Duration::from_secs(60 * 60 * 24 * 180),
                ..Default::default()
            })
            .await
        {
            Ok(stream) => stream,
            Err(err) => return Err(anyhow::anyhow!("JetStream stream error: {err}")),
        };

        let consumer = match stream
            .get_or_create_consumer(
                "executor",
                consumer::pull::Config {
                    durable_name: Some("executor".to_string()),
                    ack_policy: consumer::AckPolicy::Explicit,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(consumer) => consumer,
            Err(err) => return Err(anyhow::anyhow!("JetStream consumer create error: {err}")),
        };

        let mut messages = match consumer.messages().await {
            Ok(messages) => messages,
            Err(err) => return Err(anyhow::anyhow!("JetStream consumer error: {err}")),
        };

        while let Some(message) = messages.next().await {
            let message = match message {
                Ok(msg) => msg,
                Err(err) => {
                    error!("JetStream error: {err}");
                    continue;
                }
            };

            let envelope: CommandEnvelope<Command> = match serde_json::from_slice(&message.payload)
            {
                Ok(cmd) => cmd,
                Err(err) => {
                    error!("Failed to deserialize command: {err}");
                    if let Err(err) = message.ack().await {
                        error!("Ack failed: {err}");
                    }
                    continue;
                }
            };

            let result = self.state_repo.registrer_kommando(&envelope).await;
            match result {
                Ok(()) => {
                    if let Err(err) = message.ack().await {
                        error!("Ack failed: {err}");
                    }
                }
                Err(err) => {
                    info!("Kunne ikke lagre kommando: {err}");
                    if let Err(err) = message.ack_with(AckKind::Nak(None)).await {
                        error!("NAK failed: {err}");
                    }
                }
            }
        }

        Ok(())
    }
}
