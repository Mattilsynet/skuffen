use anyhow::Result;
use domain::model::sak::Saksnummer;
use tracing::error;
use uuid::Uuid;

pub async fn lookup_skuffen_id_fra_arkiv_id(_saksnummer: Saksnummer) -> Result<Uuid> {
    let skuffen_id = Uuid::new_v4();
    error!("TODO: Retrnerer en generert id. Database lookup ikke implementert enda!");
    Ok(skuffen_id)
}

pub async fn lookup_arkiv_id_fra_skuffen_id(_skuffen_id: Uuid) -> Result<Saksnummer> {
    let saksnummer = Saksnummer::new("1999/12345")?;
    error!("TODO: Retrnerer en generert id. Database lookup ikke implementert enda!");
    Ok(saksnummer)
}
