use async_trait::async_trait;
use uuid::Uuid;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait IdMappingRepository: Send + Sync {
    /// Registers a mapping.
    /// Returns Ok(()) if successful or if the mapping already exists (idempotent for same inputs).
    /// Returns Error if a conflict exists (e.g. same client_ref/command_id but different internal ID).
    async fn register_mapping(
        &self,
        command_id: Uuid,
        skuffen_id: Uuid,
        entity_type: String,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error>;
}
