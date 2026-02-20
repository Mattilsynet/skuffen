use anyhow::Result;
use domain::model::sak::Saksnummer;
// use tracing::error;
use uuid::Uuid;

use std::sync::Arc;
use std::sync::OnceLock;

use application::command::ports::id_mapping_port::IdMappingRepository;

static ID_MAPPING_REPO: OnceLock<Arc<dyn IdMappingRepository + Send + Sync>> = OnceLock::new();

pub fn init_id_mapping_repo(repo: Arc<dyn IdMappingRepository + Send + Sync>) {
    ID_MAPPING_REPO.set(repo).ok(); // Ignore if already set
}

fn get_repo() -> &'static Arc<dyn IdMappingRepository + Send + Sync> {
    ID_MAPPING_REPO
        .get()
        .expect("IdMappingRepository not initialized")
}

pub async fn lookup_skuffen_id_fra_arkiv_id(saksnummer: Saksnummer) -> Result<Uuid> {
    let repo = get_repo();
    let maybe_skuffen_id = repo
        .get_skuffen_id_from_arkiv_id(saksnummer.as_str())
        .await?;

    match maybe_skuffen_id {
        Some(uid) => Ok(uid),
        None => Err(anyhow::anyhow!(
            "Skuffen ID ikke funnet for arkiv_id: {}",
            saksnummer.as_str()
        )),
    }
}

pub async fn lookup_arkiv_id_fra_skuffen_id(skuffen_id: Uuid) -> Result<Saksnummer> {
    let repo = get_repo();
    let maybe_arkiv_id = repo.get_arkiv_id(skuffen_id).await?;

    match maybe_arkiv_id {
        Some(s) => Saksnummer::new(s),
        None => Err(anyhow::anyhow!(
            "Arkiv ID ikke funnet for skuffen_id: {}",
            skuffen_id
        )),
    }
}
