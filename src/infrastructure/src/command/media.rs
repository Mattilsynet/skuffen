use async_nats::jetstream::object_store::{InfoErrorKind, ObjectStore};
use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

use bytes::Bytes;

#[derive(Clone)]
pub struct MediaFile {
    pub id: Uuid,
    pub data: Vec<u8>,
    pub filename: Option<String>,
    pub content_type: Option<String>,
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
        }))
    }
}
