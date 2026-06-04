use async_trait::async_trait;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MappingEntityType {
    Sak,
    Journalpost,
    Dokument,
}

impl MappingEntityType {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Sak => "sak",
            Self::Journalpost => "journalpost",
            Self::Dokument => "dokument",
        }
    }
}

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
        skuffen_id: SkuffenSakId,
        entity_type: MappingEntityType,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error>;

    async fn register_document_mapping(
        &self,
        command_id: Uuid,
        client_reference: Uuid,
        skuffen_id: SkuffenDokumentId,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error>;

    async fn oppdater_arkiv_id_for_client_reference(
        &self,
        client_reference: Uuid,
        arkiv_id: String,
    ) -> Result<(), anyhow::Error>;

    async fn hent_arkiv_id_fra_mapping(
        &self,
        skuffen_id: SkuffenSakId,
    ) -> Result<Option<String>, anyhow::Error>;

    async fn hent_sak_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error>;

    async fn hent_journalpost_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenJournalpostId>, anyhow::Error>;

    async fn hent_dokument_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenDokumentId>, anyhow::Error>;

    async fn hent_sak_id_fra_arkiv_id_i_mapping(
        &self,
        arkiv_id: &str,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error>;

    async fn hent_eller_opprett_skuffen_id_for_arkiv_id(
        &self,
        entity_type: MappingEntityType,
        arkiv_id: &str,
    ) -> Result<SkuffenSakId, anyhow::Error>;

    async fn delete_arkiv_mapping(
        &self,
        entity_type: MappingEntityType,
        arkiv_id: &str,
    ) -> Result<(), anyhow::Error>;
}
