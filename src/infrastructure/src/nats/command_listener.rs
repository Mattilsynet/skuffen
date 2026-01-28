use crate::nats::client::NatsClient;
use application::services::ingest_command::IngestCommandService;
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{CommandEnvelope, CommandSequence, Kommando};
use tracing::{error, info};

pub struct CommandListener {
    client: NatsClient,
    service: IngestCommandService,
}

impl CommandListener {
    pub fn new(client: NatsClient, service: IngestCommandService) -> Self {
        Self { client, service }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let subject = "skuffen.command.submit";
        info!("Listening for command batches on '{}'", subject);

        // Queue group 'skuffen-command-processor' for load balancing if scaled
        let mut sub = self
            .client
            .inner()
            .queue_subscribe(subject.to_string(), "skuffen-command-processor".to_string())
            .await?;

        while let Some(msg) = sub.next().await {
            info!("Received command batch");

            let reply_subject = match msg.reply.clone() {
                Some(r) => r,
                None => {
                    error!("Command batch has no reply subject. Ignoring.");
                    continue;
                }
            };

            // Deserialize Vec<CommandEnvelope<Kommando>>
            let commands: Vec<CommandEnvelope<Kommando>> =
                match serde_json::from_slice(&msg.payload) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to deserialize commands: {e}");
                        // Reply error
                        let _ = self
                            .client
                            .inner()
                            .publish(reply_subject, format!("Invalid payload: {e}").into())
                            .await;
                        continue;
                    }
                };

            // Validate sequence (Infrastructure responsibility: Parse/Validate input structure)
            let sequence = match CommandSequence::try_from(commands) {
                Ok(seq) => seq,
                Err(e) => {
                    error!("Invalid command sequence: {e}");
                    let _ = self
                        .client
                        .inner()
                        .publish(reply_subject, format!("Invalid sequence: {e}").into())
                        .await;
                    continue;
                }
            };

            // Ingest
            match self.service.handle(sequence).await {
                Ok(_) => {
                    // Reply OK
                    let _ = self
                        .client
                        .inner()
                        .publish(reply_subject, "OK".into())
                        .await;
                }
                Err(e) => {
                    error!("Failed to process commands: {e}");
                    let _ = self
                        .client
                        .inner()
                        .publish(reply_subject, format!("Error: {e}").into())
                        .await;
                }
            }
        }
        Ok(())
    }
}
