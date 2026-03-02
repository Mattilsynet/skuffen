use std::sync::Arc;

use futures::StreamExt;
use lib_nats::chunked_upload::receiver::ChunkedUploadAssembler;
use tracing::{Instrument, error, info};
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

    #[tracing::instrument(skip_all, name = "nats.media_listener")]
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

            let span = tracing::info_span!(
                "media.assemble",
                subject = %message.subject,
                traceparent = tracing::field::Empty
            );
            if let Some(headers) = message.headers.as_ref()
                && let Some(parent) = headers.get("traceparent")
            {
                span.record("traceparent", tracing::field::display(parent.as_str()));
            }
            let payload = match async { assembler.push(&message) }.instrument(span).await {
                Ok(Some(payload)) => payload,
                Ok(None) => continue,
                Err(err) => {
                    error!("Chunk assembly failed: {err}");
                    self.publish_error(&reply_subject, err.to_string()).await;
                    continue;
                }
            };

            let file_id = match Uuid::parse_str(payload.upload_id.as_str()) {
                Ok(id) => id,
                Err(err) => {
                    error!("Invalid upload id: {err}");
                    self.publish_error(&reply_subject, "Invalid upload id")
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
                self.publish_error(&reply_subject, err.to_string()).await;
                continue;
            }

            self.publish_ok(&reply_subject, file_id).await;
        }

        Ok(())
    }

    async fn publish_error(&self, reply_subject: &str, message: impl Into<String>) {
        let response = NatsResponse::<()>::Error {
            message: message.into(),
        };
        let payload = serde_json::to_vec(&response).unwrap_or_default();
        let subject = reply_subject.to_string();
        let _ = self.client.inner().publish(subject, payload.into()).await;
    }

    async fn publish_ok(&self, reply_subject: &str, file_id: Uuid) {
        let response = NatsResponse::Ok(file_id);
        let payload = serde_json::to_vec(&response).unwrap_or_default();
        let subject = reply_subject.to_string();
        let _ = self.client.inner().publish(subject, payload.into()).await;
    }
}
