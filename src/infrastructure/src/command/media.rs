use application::command::ports::dokument_lager_port::{
    DokumentFil, DokumentLager, DokumentMetadata,
};
use async_nats::jetstream::object_store::{InfoErrorKind, ObjectStore, UpdateMetadata};
use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use bytes::Bytes;
use tracing::{error, info, warn};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaMetadata {
    pub origin: Option<String>,
    pub source_template_reference: Option<Uuid>,
    pub source_document_id: Option<Uuid>,
    pub source_command_id: Option<Uuid>,
    pub render_timestamp: Option<String>,
}

#[derive(Clone)]
pub struct MediaFile {
    pub id: Uuid,
    pub data: Vec<u8>,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub metadata: MediaMetadata,
}

#[async_trait]
pub trait MediaStore: Send + Sync {
    async fn save(&self, file: MediaFile) -> Result<(), anyhow::Error>;
    async fn exists(&self, id: Uuid) -> Result<bool, anyhow::Error>;
    async fn get(&self, id: Uuid) -> Result<Option<MediaFile>, anyhow::Error>;
}

#[derive(Clone)]
pub struct ObjectStoreMediaStore {
    store: ObjectStore,
}

impl ObjectStoreMediaStore {
    pub fn new(store: ObjectStore) -> Self {
        Self { store }
    }
}

#[async_trait]
impl MediaStore for ObjectStoreMediaStore {
    async fn save(&self, file: MediaFile) -> Result<(), anyhow::Error> {
        let object_name = file.id.to_string();
        let byte_len = file.data.len();
        let content_type = file.content_type.as_deref().unwrap_or("unknown");
        let origin = file.metadata.origin.as_deref().unwrap_or("unknown");
        let source_template_reference =
            format_optional_uuid(file.metadata.source_template_reference);
        let source_document_id = format_optional_uuid(file.metadata.source_document_id);
        let source_command_id = format_optional_uuid(file.metadata.source_command_id);

        info!(
            event = "media_save_start",
            operation = "save",
            media_id = %file.id,
            byte_len,
            content_type,
            origin,
            source_template_reference = %source_template_reference,
            source_document_id = %source_document_id,
            source_command_id = %source_command_id,
            "media save started: {} bytes {}", byte_len, content_type
        );

        let mut reader = std::io::Cursor::new(Bytes::from(file.data));
        if let Err(err) = self.store.put(object_name.as_str(), &mut reader).await {
            let error_message = sanitize_media_error(&err.to_string());
            error!(
                event = "media_save_failed",
                operation = "save",
                media_id = %object_name,
                byte_len,
                content_type,
                origin,
                source_template_reference = %source_template_reference,
                source_document_id = %source_document_id,
                source_command_id = %source_command_id,
                error_category = "object_store_put_failed",
                error_message = %error_message,
                "media save failed: object_store_put_failed"
            );
            return Err(anyhow::anyhow!(err));
        }
        if let Some(metadata) = object_metadata(&object_name, &file.metadata)
            && let Err(err) = self
                .store
                .update_metadata(object_name.as_str(), metadata)
                .await
        {
            let error_message = sanitize_media_error(&err.to_string());
            error!(
                event = "media_save_failed",
                operation = "save_metadata",
                media_id = %object_name,
                byte_len,
                content_type,
                origin,
                source_template_reference = %source_template_reference,
                source_document_id = %source_document_id,
                source_command_id = %source_command_id,
                error_category = "object_store_metadata_failed",
                error_message = %error_message,
                "media save failed: object_store_metadata_failed"
            );
            return Err(anyhow::anyhow!(err));
        }
        info!(
            event = "media_save_ok",
            operation = "save",
            media_id = %object_name,
            byte_len,
            content_type,
            origin,
            source_template_reference = %source_template_reference,
            source_document_id = %source_document_id,
            source_command_id = %source_command_id,
            "media save ok: {} bytes {}", byte_len, content_type
        );
        Ok(())
    }

    async fn exists(&self, id: Uuid) -> Result<bool, anyhow::Error> {
        let object_name = id.to_string();
        match self.store.info(object_name.as_str()).await {
            Ok(_) => Ok(true),
            Err(err) if matches!(err.kind(), InfoErrorKind::NotFound) => Ok(false),
            Err(err) => Err(anyhow::anyhow!(err)),
        }
    }

    async fn get(&self, id: Uuid) -> Result<Option<MediaFile>, anyhow::Error> {
        let object_name = id.to_string();
        info!(
            event = "media_get_start",
            operation = "get",
            media_id = %id,
            "media get started"
        );
        let mut object = match self.store.get(object_name.as_str()).await {
            Ok(object) => object,
            Err(err)
                if matches!(
                    err.kind(),
                    async_nats::jetstream::object_store::GetErrorKind::NotFound
                ) =>
            {
                warn!(
                    event = "media_get_missing",
                    operation = "get",
                    media_id = %id,
                    error_category = "object_store_not_found",
                    "media get missing: object_store_not_found"
                );
                return Ok(None);
            }
            Err(err) => {
                let error_message = sanitize_media_error(&err.to_string());
                error!(
                    event = "media_get_failed",
                    operation = "get",
                    media_id = %id,
                    error_category = "object_store_get_failed",
                    error_message = %error_message,
                    "media get failed: object_store_get_failed"
                );
                return Err(anyhow::anyhow!(err));
            }
        };

        let mut data = Vec::new();
        if let Err(err) = object.read_to_end(&mut data).await {
            let error_message = sanitize_media_error(&err.to_string());
            error!(
                event = "media_get_failed",
                operation = "read",
                media_id = %id,
                error_category = "object_store_read_failed",
                error_message = %error_message,
                "media get failed: object_store_read_failed"
            );
            return Err(anyhow::anyhow!(err));
        }
        let info = object.info;
        let byte_len = data.len();
        info!(
            event = "media_get_ok",
            operation = "get",
            media_id = %id,
            byte_len,
            object_name = %info.name,
            "media get ok: {} bytes", byte_len
        );

        Ok(Some(MediaFile {
            id,
            data,
            filename: Some(info.name),
            content_type: None,
            metadata: MediaMetadata::default(),
        }))
    }
}

fn format_optional_uuid(id: Option<Uuid>) -> String {
    id.map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn sanitize_media_error(detail: &str) -> String {
    let normalized = detail
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let redacted = redact_media_error_tokens(&normalized);
    const MAX_MEDIA_ERROR_DETAIL: usize = 300;
    if redacted.chars().count() <= MAX_MEDIA_ERROR_DETAIL {
        redacted
    } else {
        format!(
            "{}…",
            redacted
                .chars()
                .take(MAX_MEDIA_ERROR_DETAIL)
                .collect::<String>()
        )
    }
}

fn redact_media_error_tokens(detail: &str) -> String {
    let mut redacted = Vec::new();
    let mut redact_next = 0;
    for token in detail.split_whitespace() {
        if redact_next > 0 {
            redacted.push("redacted".to_string());
            redact_next -= 1;
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if is_sensitive_media_token(&lower) {
            redacted.push(redact_media_token(token));
            redact_next = sensitive_media_following_token_count(token, &lower);
        } else {
            redacted.push(token.to_string());
        }
    }
    redacted.join(" ")
}

fn is_sensitive_media_token(lower: &str) -> bool {
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

fn sensitive_media_following_token_count(token: &str, lower: &str) -> usize {
    if lower.contains("authorization") && token.ends_with(':') {
        2
    } else if token.ends_with(':') || lower == "bearer" || lower == "basic" {
        1
    } else {
        0
    }
}

fn redact_media_token(token: &str) -> String {
    if let Some((key, _)) = token.split_once('=') {
        format!("{key}=redacted")
    } else if let Some((key, _)) = token.split_once(':') {
        format!("{key}:redacted")
    } else {
        "redacted".to_string()
    }
}

fn object_metadata(object_name: &str, metadata: &MediaMetadata) -> Option<UpdateMetadata> {
    let mut values = std::collections::HashMap::new();
    insert_opt(&mut values, "origin", metadata.origin.clone());
    insert_opt(
        &mut values,
        "source_template_reference",
        metadata.source_template_reference.map(|id| id.to_string()),
    );
    insert_opt(
        &mut values,
        "source_document_id",
        metadata.source_document_id.map(|id| id.to_string()),
    );
    insert_opt(
        &mut values,
        "source_command_id",
        metadata.source_command_id.map(|id| id.to_string()),
    );
    insert_opt(
        &mut values,
        "render_timestamp",
        metadata.render_timestamp.clone(),
    );

    if values.is_empty() {
        return None;
    }

    Some(UpdateMetadata {
        name: object_name.to_string(),
        description: None,
        metadata: values,
        headers: None,
    })
}

fn insert_opt(
    values: &mut std::collections::HashMap<String, String>,
    key: &str,
    value: Option<String>,
) {
    if let Some(value) = value {
        values.insert(key.to_string(), value);
    }
}

#[async_trait]
impl DokumentLager for ObjectStoreMediaStore {
    async fn save(&self, file: DokumentFil) -> Result<(), anyhow::Error> {
        MediaStore::save(
            self,
            MediaFile {
                id: file.id,
                data: file.data,
                filename: file.filename,
                content_type: file.content_type,
                metadata: MediaMetadata {
                    origin: file.metadata.origin,
                    source_template_reference: file.metadata.source_template_reference,
                    source_document_id: file.metadata.source_document_id,
                    source_command_id: file.metadata.source_command_id,
                    render_timestamp: file.metadata.render_timestamp,
                },
            },
        )
        .await
    }

    async fn get(&self, id: Uuid) -> Result<Option<DokumentFil>, anyhow::Error> {
        Ok(MediaStore::get(self, id).await?.map(|file| DokumentFil {
            id: file.id,
            data: file.data,
            filename: file.filename,
            content_type: file.content_type,
            metadata: DokumentMetadata {
                origin: file.metadata.origin,
                source_template_reference: file.metadata.source_template_reference,
                source_document_id: file.metadata.source_document_id,
                source_command_id: file.metadata.source_command_id,
                render_timestamp: file.metadata.render_timestamp,
            },
        }))
    }
}
