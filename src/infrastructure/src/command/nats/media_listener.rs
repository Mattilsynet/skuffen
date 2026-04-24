use std::sync::Arc;

use async_nats::Message;
use futures::StreamExt;
use lib_nats::chunked_upload::receiver::ChunkedUploadAssembler;
use tracing::{error, info};
use uuid::Uuid;

use crate::command::media::{MediaFile, MediaStore};
use crate::nats::client::NatsClient;
use crate::nats::nats_response::NatsResponse;
use crate::nats::supervisor::TaskSupervisor;

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
        let supervisor = TaskSupervisor::critical("media_listener", 3);
        supervisor.run(|| self.run_once()).await
    }

    async fn run_once(&self) -> anyhow::Result<()> {
        let subject = "arkiv.arkiver.media";
        info!("Listening for media uploads on '{}'", subject);

        let mut sub = self
            .client
            .inner()
            .queue_subscribe(subject.to_string(), "skuffen-media-processor".to_string())
            .await?;

        let mut assembler = ChunkedUploadAssembler::default();

        while let Some(message) = sub.next().await {
            self.process_message(&mut assembler, message).await;
        }

        Err(anyhow::anyhow!(
            "media listener subscription ended unexpectedly"
        ))
    }

    #[tracing::instrument(
        skip_all,
        name = "media.assemble",
        fields(subject = %message.subject)
    )]
    async fn process_message(&self, assembler: &mut ChunkedUploadAssembler, message: Message) {
        let reply_subject = match message.reply.clone() {
            Some(reply) => reply,
            None => {
                error!("Media upload message has no reply subject. Ignoring.");
                return;
            }
        };

        crate::telemetry::set_parent_from_nats_headers(message.headers.as_ref());

        let payload = match assembler.push(&message) {
            Ok(Some(payload)) => payload,
            Ok(None) => return,
            Err(err) => {
                error!("Chunk assembly failed: {err}");
                self.publish_error(&reply_subject, "Internal error").await;
                return;
            }
        };

        let file_id = match Uuid::parse_str(payload.upload_id.as_str()) {
            Ok(id) => id,
            Err(err) => {
                error!("Invalid upload id: {err}");
                self.publish_error(&reply_subject, "Invalid upload id")
                    .await;
                return;
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
            self.publish_error(&reply_subject, "Internal error").await;
            return;
        }

        self.publish_ok(&reply_subject, file_id).await;
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
