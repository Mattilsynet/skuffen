use std::sync::Arc;

use async_nats::Message;
use futures::StreamExt;
use lib_nats::chunked_upload::receiver::ChunkedUploadAssembler;
use tracing::{error, info};
use uuid::Uuid;

use crate::command::media::{MediaFile, MediaMetadata, MediaStore};
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
                error!(
                    event = "media_upload_failed",
                    error_category = "chunk_assembly_failed",
                    error_message = %sanitize_media_listener_error(&err.to_string()),
                    "media upload failed: chunk_assembly_failed"
                );
                self.publish_error(&reply_subject, "Internal error").await;
                return;
            }
        };

        let file_id = match Uuid::parse_str(payload.upload_id.as_str()) {
            Ok(id) => id,
            Err(err) => {
                error!(
                    event = "media_upload_failed",
                    error_category = "invalid_upload_id",
                    error_message = %sanitize_media_listener_error(&err.to_string()),
                    "media upload failed: invalid_upload_id"
                );
                self.publish_error(&reply_subject, "Invalid upload id")
                    .await;
                return;
            }
        };

        let byte_len = payload.data.len();
        let content_type = payload
            .content_type
            .as_deref()
            .unwrap_or("unknown")
            .to_string();
        let filename_ext = safe_filename_extension(payload.filename.as_deref());

        let file = MediaFile {
            id: file_id,
            data: payload.data,
            filename: payload.filename,
            content_type: payload.content_type,
            metadata: MediaMetadata::default(),
        };

        if let Err(err) = self.store.save(file).await {
            error!(
                event = "media_upload_failed",
                upload_id = %file_id,
                byte_len,
                content_type = %content_type,
                filename_ext = %filename_ext,
                error_category = "media_store_save_failed",
                error_message = %sanitize_media_listener_error(&err.to_string()),
                "media upload failed: media_store_save_failed"
            );
            self.publish_error(&reply_subject, "Internal error").await;
            return;
        }

        info!(
            event = "media_upload_ok",
            upload_id = %file_id,
            byte_len,
            content_type = %content_type,
            filename_ext = %filename_ext,
            "media upload ok: {} bytes {}", byte_len, content_type
        );
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

fn safe_filename_extension(filename: Option<&str>) -> String {
    filename
        .and_then(|name| name.rsplit_once('.').map(|(_, ext)| ext))
        .filter(|ext| ext.len() <= 16 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .map(|ext| format!(".{ext}"))
        .unwrap_or_else(|| "unknown".to_string())
}

fn sanitize_media_listener_error(detail: &str) -> String {
    let normalized = detail
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let redacted = redact_media_listener_tokens(&normalized);
    const MAX_MEDIA_LISTENER_ERROR_DETAIL: usize = 240;
    if redacted.chars().count() <= MAX_MEDIA_LISTENER_ERROR_DETAIL {
        redacted
    } else {
        format!(
            "{}…",
            redacted
                .chars()
                .take(MAX_MEDIA_LISTENER_ERROR_DETAIL)
                .collect::<String>()
        )
    }
}

fn redact_media_listener_tokens(detail: &str) -> String {
    let mut redacted = Vec::new();
    let mut redact_next = 0;
    for token in detail.split_whitespace() {
        if redact_next > 0 {
            redacted.push("redacted".to_string());
            redact_next -= 1;
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if is_sensitive_media_listener_token(&lower) {
            redacted.push(redact_media_listener_token(token));
            redact_next = sensitive_media_listener_following_token_count(token, &lower);
        } else {
            redacted.push(token.to_string());
        }
    }
    redacted.join(" ")
}

fn is_sensitive_media_listener_token(lower: &str) -> bool {
    lower.contains("authorization")
        || lower == "bearer"
        || lower.starts_with("bearer=")
        || lower == "basic"
        || lower.starts_with("basic=")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("passwd")
        || lower.contains("credential")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("x-api-key")
}

fn sensitive_media_listener_following_token_count(token: &str, lower: &str) -> usize {
    if lower.contains("authorization") && token.ends_with(':') {
        2
    } else if token.ends_with(':') || lower == "bearer" || lower == "basic" {
        1
    } else {
        0
    }
}

fn redact_media_listener_token(token: &str) -> String {
    if let Some((key, _)) = token.split_once('=') {
        format!("{key}=redacted")
    } else if let Some((key, _)) = token.split_once(':') {
        format!("{key}:redacted")
    } else {
        "redacted".to_string()
    }
}
