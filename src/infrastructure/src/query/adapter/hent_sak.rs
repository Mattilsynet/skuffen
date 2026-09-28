use application::query::services::hent_sak::{SakRepository, SaksnummerOppslag};
use async_trait::async_trait;
use domain::model::sak::{Sak, Saksnummer};
use uuid::Uuid;

use crate::query::mapping::{
    self,
    lookup::entitet_queries::{
        lookup_arkiv_id_fra_skuffen_id, lookup_sak_skuffen_id_fra_client_reference,
    },
};

#[derive(Debug)]
pub struct SikriRepository;

#[async_trait]
impl SakRepository for SikriRepository {
    #[tracing::instrument(
        skip_all,
        name = "sak.hent",
        fields(saksnummer = %saksnummer.as_str(), inkluder_journalposter)
    )]
    async fn hent_sak(
        &self,
        saksnummer: Saksnummer,
        inkluder_journalposter: bool,
    ) -> Result<Sak, anyhow::Error> {
        let sak_respons =
            sikri_client::hent_sak(saksnummer.as_str(), "SKUFFEN", inkluder_journalposter).await?;
        mapping::fra_sikri_til_domene::sak::from_sikri_sak_to_domain_sak(sak_respons)
    }
}

/// Løser klientreferanse til saksnummer gjennom Skuffens entitetsregister.
#[derive(Debug)]
pub struct EntitetSaksnummerOppslag;

#[async_trait]
impl SaksnummerOppslag for EntitetSaksnummerOppslag {
    async fn saksnummer_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<Saksnummer, anyhow::Error> {
        let skuffen_id = lookup_sak_skuffen_id_fra_client_reference(client_reference).await?;
        lookup_arkiv_id_fra_skuffen_id(skuffen_id).await
    }
}
