use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::Command;
use uuid::Uuid;

#[async_trait]
pub trait IdMappingRepository: Send + Sync {
    /// Checks if a command with the given ID has already been fully processed (idempotency check).
    async fn has_processed_command(&self, command_id: Uuid) -> Result<bool, anyhow::Error>;

    /// Registers a mapping.
    /// Returns Ok(()) if successful or if the mapping already exists (idempotent for same inputs).
    /// Returns Error if a conflict exists (e.g. same client_ref/command_id but different internal ID).
    async fn register_mapping(
        &self,
        command_id: Uuid,
        client_reference: Uuid,
        skuffen_id: Uuid,
        command: &Command,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error>;

    async fn register_document_mapping(
        &self,
        command_id: Uuid,
        client_reference: Uuid,
        skuffen_id: Uuid,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error>;

    async fn oppdater_arkiv_id_for_client_reference(
        &self,
        client_reference: Uuid,
        arkiv_id: String,
    ) -> Result<(), anyhow::Error>;

    async fn get_arkiv_id(&self, skuffen_id: Uuid) -> Result<Option<String>, anyhow::Error>;

    async fn get_skuffen_id(&self, client_reference: Uuid) -> Result<Option<Uuid>, anyhow::Error>;

    async fn get_skuffen_id_from_arkiv_id(
        &self,
        arkiv_id: &str,
    ) -> Result<Option<Uuid>, anyhow::Error>;

    async fn ensure_arkiv_mapping(
        &self,
        entity_type: &str,
        arkiv_id: &str,
    ) -> Result<Uuid, anyhow::Error>;

    async fn delete_arkiv_mapping(
        &self,
        entity_type: &str,
        arkiv_id: &str,
    ) -> Result<(), anyhow::Error>;
}
