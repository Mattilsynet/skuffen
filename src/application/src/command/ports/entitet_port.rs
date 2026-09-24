use async_trait::async_trait;
use domain::eksekvering::operasjon::EntitetType;
use uuid::Uuid;

/// Identitetstabellen (SKU-0016 R11). Master for `skuffen_id`.
///
/// Egen tabell fordi id-ene mintes ved ingest, før validering — en kommando kan
/// få id-er og så bli avvist, uten at noen state-rad oppstår.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NyEntitet {
    pub skuffen_id: Uuid,
    pub entitet_type: EntitetType,
    pub client_reference: Option<Uuid>,
    pub arkiv_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entitet {
    pub skuffen_id: Uuid,
    pub entitet_type: EntitetType,
    pub client_reference: Option<Uuid>,
    pub arkiv_id: Option<String>,
}

#[async_trait]
pub trait EntitetRepository: Send + Sync {
    /// Idempotent: en kjent `client_reference` gir den eksisterende
    /// `skuffen_id`, slik at en replay gjenbruker id-ene fra første forsøk.
    async fn registrer(&self, entitet: NyEntitet) -> Result<Uuid, anyhow::Error>;

    async fn hent_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<Entitet>, anyhow::Error>;

    /// Rent oppslag, uten bivirkninger. Brukes fra query-siden.
    async fn hent_for_arkiv_id(
        &self,
        entitet_type: EntitetType,
        arkiv_id: &str,
    ) -> Result<Option<Entitet>, anyhow::Error>;

    /// Brer en ekstern arkiv-id inn i vår identitetsmodell.
    async fn hent_eller_opprett_for_arkiv_id(
        &self,
        entitet_type: EntitetType,
        arkiv_id: &str,
    ) -> Result<Uuid, anyhow::Error>;

    async fn hent_arkiv_id(&self, skuffen_id: Uuid) -> Result<Option<String>, anyhow::Error>;
}
