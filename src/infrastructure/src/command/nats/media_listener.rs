use std::sync::Arc;

use futures::StreamExt;
use lib_nats::chunked_upload::receiver::ChunkedUploadAssembler;
use tracing::{error, info};
use uuid::Uuid;

use crate::command::media::{MediaFile, MediaStore};
use crate::nats::client::NatsClient;
use crate::nats::nats_response::NatsResponse;

pub struct MediaListener {
    client: NatsClient,
    store: Arc<dyn MediaStore>,
}

impl MediaListener {
    pub fn new(client: NatsClient, store: Arc<dyn MediaStore>) -> Self {
        Self { client, store }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let subject = "arkiv.arkiver.media";
        info!("Listening for media uploads on '{}'", subject);

        let mut sub = self
            .client
            .inner()
            .queue_subscribe(subject.to_string(), "skuffen-media-processor".to_string())
            .await?;

        let mut assembler = ChunkedUploadAssembler::default();

        while let Some(message) = sub.next().await {
            let reply_subject = match message.reply.clone() {
                Some(reply) => reply,
                None => {
                    error!("Media upload message has no reply subject. Ignoring.");
                    continue;
                }
            };

            let payload = match assembler.push(&message) {
                Ok(Some(payload)) => payload,
                Ok(None) => continue,
                Err(err) => {
                    error!("Chunk assembly failed: {err}");
                    let response = NatsResponse::<()>::Error {
                        message: err.to_string(),
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

            let file_id = match Uuid::parse_str(payload.upload_id.as_str()) {
                Ok(id) => id,
                Err(err) => {
                    error!("Invalid upload id: {err}");
                    let response = NatsResponse::<()>::Error {
                        message: "Invalid upload id".to_string(),
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

            let file = MediaFile {
                id: file_id,
                data: payload.data,
                filename: payload.filename,
                content_type: payload.content_type,
            };

            if let Err(err) = self.store.save(file).await {
                error!("Failed to store media: {err}");
                let response = NatsResponse::<()>::Error {
                    message: err.to_string(),
                };
                let payload = serde_json::to_vec(&response).unwrap_or_default();
                let _ = self
                    .client
                    .inner()
                    .publish(reply_subject, payload.into())
                    .await;
                continue;
            }

            let response = NatsResponse::Ok(file_id);
            let response_payload = serde_json::to_vec(&response).unwrap_or_default();
            let _ = self
                .client
                .inner()
                .publish(reply_subject, response_payload.into())
                .await;
        }

        Ok(())
    }
}
