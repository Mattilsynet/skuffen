use application::command::ports::dokument_lager_port::{
    DokumentFil, DokumentLager, DokumentMetadata,
};
use async_nats::jetstream::object_store::{InfoErrorKind, ObjectStore, UpdateMetadata};
use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use bytes::Bytes;

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
        let mut reader = std::io::Cursor::new(Bytes::from(file.data));
        self.store.put(object_name.as_str(), &mut reader).await?;
        if let Some(metadata) = object_metadata(&object_name, &file.metadata) {
            self.store
                .update_metadata(object_name.as_str(), metadata)
                .await?;
        }
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
        let mut object = match self.store.get(object_name.as_str()).await {
            Ok(object) => object,
            Err(err)
                if matches!(
                    err.kind(),
                    async_nats::jetstream::object_store::GetErrorKind::NotFound
                ) =>
            {
                return Ok(None);
            }
            Err(err) => return Err(anyhow::anyhow!(err)),
        };

        let mut data = Vec::new();
        object.read_to_end(&mut data).await?;
        let info = object.info;

        Ok(Some(MediaFile {
            id,
            data,
            filename: Some(info.name),
            content_type: None,
            metadata: MediaMetadata::default(),
        }))
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
