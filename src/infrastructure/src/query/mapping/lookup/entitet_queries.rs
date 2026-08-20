use anyhow::Result;
use application::command::ports::entitet_port::EntitetRepository;
use domain::eksekvering::operasjon::EntitetType;
use domain::model::sak::Saksnummer;
// use tracing::error;
use uuid::Uuid;

use std::sync::{Arc, OnceLock, RwLock};

static ENTITET_REPO: OnceLock<RwLock<Option<Arc<dyn EntitetRepository + Send + Sync>>>> =
    OnceLock::new();

pub fn init_entitet_repo(repo: Arc<dyn EntitetRepository + Send + Sync>) {
    let repo_slot = ENTITET_REPO.get_or_init(|| RwLock::new(None));
    *repo_slot.write().expect("EntitetRepository lock poisoned") = Some(repo);
}

fn get_repo() -> Arc<dyn EntitetRepository + Send + Sync> {
    ENTITET_REPO
        .get()
        .expect("EntitetRepository not initialized")
        .read()
        .expect("EntitetRepository lock poisoned")
        .as_ref()
        .cloned()
        .expect("EntitetRepository not initialized")
}

pub async fn lookup_skuffen_id_fra_arkiv_id(saksnummer: Saksnummer) -> Result<Uuid> {
    let repo = get_repo();
    let maybe_entitet = repo
        .hent_for_arkiv_id(EntitetType::Sak, saksnummer.as_str())
        .await?;

    match maybe_entitet {
        Some(entitet) => Ok(entitet.skuffen_id),
        None => Err(anyhow::anyhow!(
            "Skuffen ID ikke funnet for arkiv_id: {}",
            saksnummer.as_str()
        )),
    }
}

pub async fn lookup_arkiv_id_fra_skuffen_id(skuffen_id: Uuid) -> Result<Saksnummer> {
    let repo = get_repo();
    let maybe_arkiv_id = repo.hent_arkiv_id(skuffen_id).await?;

    match maybe_arkiv_id {
        Some(s) => Saksnummer::new(s),
        None => Err(anyhow::anyhow!(
            "Arkiv ID ikke funnet for skuffen_id: {}",
            skuffen_id
        )),
    }
}
