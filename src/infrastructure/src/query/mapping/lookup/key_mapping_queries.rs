use anyhow::Result;
use application::command::ports::id_mapping_port::IdMappingRepository;
use domain::eksekvering::id::SkuffenSakId;
use domain::model::sak::Saksnummer;
// use tracing::error;
use uuid::Uuid;

use std::sync::{Arc, OnceLock, RwLock};

static ID_MAPPING_REPO: OnceLock<RwLock<Option<Arc<dyn IdMappingRepository + Send + Sync>>>> =
    OnceLock::new();

pub fn init_id_mapping_repo(repo: Arc<dyn IdMappingRepository + Send + Sync>) {
    let repo_slot = ID_MAPPING_REPO.get_or_init(|| RwLock::new(None));
    *repo_slot
        .write()
        .expect("IdMappingRepository lock poisoned") = Some(repo);
}

fn get_repo() -> Arc<dyn IdMappingRepository + Send + Sync> {
    ID_MAPPING_REPO
        .get()
        .expect("IdMappingRepository not initialized")
        .read()
        .expect("IdMappingRepository lock poisoned")
        .as_ref()
        .cloned()
        .expect("IdMappingRepository not initialized")
}

pub async fn lookup_skuffen_id_fra_arkiv_id(saksnummer: Saksnummer) -> Result<Uuid> {
    let repo = get_repo();
    let maybe_skuffen_id = repo
        .hent_sak_id_fra_arkiv_id_i_mapping(saksnummer.as_str())
        .await?;

    match maybe_skuffen_id {
        Some(uid) => Ok(Uuid::from(uid)),
        None => Err(anyhow::anyhow!(
            "Skuffen ID ikke funnet for arkiv_id: {}",
            saksnummer.as_str()
        )),
    }
}

pub async fn lookup_arkiv_id_fra_skuffen_id(skuffen_id: Uuid) -> Result<Saksnummer> {
    let repo = get_repo();
    let maybe_arkiv_id = repo
        .hent_arkiv_id_fra_mapping(SkuffenSakId::from(skuffen_id))
        .await?;

    match maybe_arkiv_id {
        Some(s) => Saksnummer::new(s),
        None => Err(anyhow::anyhow!(
            "Arkiv ID ikke funnet for skuffen_id: {}",
            skuffen_id
        )),
    }
}
