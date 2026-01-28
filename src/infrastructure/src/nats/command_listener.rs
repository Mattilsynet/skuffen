use crate::nats::client::NatsClient;
use crate::nats::nats_response::NatsResponse;
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
                        let response = NatsResponse::<()>::Error {
                            message: format!("Invalid payload: {e}"),
                        };
                        let payload = serde_json::to_vec(&response).unwrap_or_default();
                        let _ = self
                            .client
                            .inner()
                            .publish(reply_subject, payload.into())
                            .await;
                        continue;
                    }
                };

            // Validate sequence (Infrastructure responsibility: Parse/Validate input structure)
            let sequence = match CommandSequence::try_from(commands) {
                Ok(seq) => seq,
                Err(e) => {
                    error!("Invalid command sequence: {e}");
                    let response = NatsResponse::<()>::Error {
                        message: format!("Invalid sequence: {e}"),
                    };
                    let payload = serde_json::to_vec(&response).unwrap_or_default();
                    let _ = self
                        .client
                        .inner()
                        .publish(reply_subject, payload.into())
                        .await;
                    continue;
                }
            };

            // Ingest
            // Ingest
            match self.service.handle(sequence).await {
                Ok(_) => {
                    // Reply OK
                    let response = NatsResponse::Ok(());
                    let payload = serde_json::to_vec(&response).unwrap_or_default();
                    let _ = self
                        .client
                        .inner()
                        .publish(reply_subject, payload.into())
                        .await;
                }
                Err(e) => {
                    error!("Failed to process commands: {e}");
                    let response = NatsResponse::<()>::Error {
                        message: e.to_string(),
                    };
                    let payload = serde_json::to_vec(&response).unwrap_or_default();
                    let _ = self
                        .client
                        .inner()
                        .publish(reply_subject, payload.into())
                        .await;
                }
            }
        }
        Ok(())
    }
}
