use async_trait::async_trait;
use async_nats::jetstream::object_store::ObjectStore;
use uuid::Uuid;

use lib_nats::error::Error as NatsError;
use lib_nats::object_store;

#[derive(Debug, Clone)]
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
}

#[derive(Debug, Clone)]
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
        object_store::store_bytes(&self.store, &object_name, &file.data).await?;
        Ok(())
    }

    async fn exists(&self, id: Uuid) -> Result<bool, anyhow::Error> {
        let object_name = id.to_string();
        match object_store::object_info(&self.store, &object_name).await {
            Ok(_) => Ok(true),
            Err(NatsError::NotFoundError(_)) => Ok(false),
            Err(err) => Err(anyhow::anyhow!(err)),
        }
    }
}
