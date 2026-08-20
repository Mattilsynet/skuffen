use async_trait::async_trait;
use domain::eksekvering::operasjon::EntitetType;
use uuid::Uuid;

/// Identitetstabellen (SKU-0016 R11). Master for `skuffen_id`.
///
/// Består som egen tabell fordi `skuffen_id` mintes ved ingest, før vi vet om
/// entiteten noensinne får en state-rad: en kommando kan mottas, id-er deles
/// ut, og så feile validering.
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
    /// Idempotent. Returnerer den effektive `skuffen_id`-en: er
    /// `client_reference` sett før, vinner den eksisterende raden. Det gjør at
    /// en replay etter dispatch-feil gjenbruker id-ene fra første forsøk i
    /// stedet for å minte nye.
    async fn registrer(&self, entitet: NyEntitet) -> Result<Uuid, anyhow::Error>;

    async fn hent_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<Entitet>, anyhow::Error>;

    /// Rent oppslag. Oppretter aldri — brukes fra query-siden, som ikke skal
    /// ha bivirkninger.
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
