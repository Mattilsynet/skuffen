use anyhow::Result;
use lib_schemas::skuffen::{
    query::queries::SakKey,
    sak::{Saksnummer, Saksstatus},
};

use crate::mapping::lookup::key_mapping_queries::{
    lookup_arkiv_id_fra_skuffen_id, lookup_skuffen_id_fra_arkiv_id,
};

pub async fn from_dto_sak_key_to_domain(dto_sak_key: SakKey) -> Result<domain::model::sak::SakKey> {
    let key = match dto_sak_key {
        SakKey::SkuffenId(uuid) => domain::model::sak::SakKey {
            skuffen_id: uuid,
            arkiv_id: Some(lookup_arkiv_id_fra_skuffen_id(uuid).await?),
        },
        SakKey::ArkivId(saksnummer) => {
            let snr = from_dto_saksnumer_to_domain(saksnummer)?;
            domain::model::sak::SakKey {
                skuffen_id: lookup_skuffen_id_fra_arkiv_id(snr.clone()).await?,
                arkiv_id: Some(snr),
            }
        }
    };
    Ok(key)
}

fn from_dto_saksnumer_to_domain(
    dto_saksnummer: Saksnummer,
) -> Result<domain::model::sak::Saksnummer> {
    domain::model::sak::Saksnummer::new(dto_saksnummer.as_str())
}

#[allow(dead_code)]
fn from_dto_sakstatus_to_domain(dto_saksstatus: Saksstatus) -> domain::model::sak::Saksstatus {
    match dto_saksstatus {
        Saksstatus::UnderBehandling => domain::model::sak::Saksstatus::UnderBehandling,
        Saksstatus::Ferdig => domain::model::sak::Saksstatus::Ferdig,
        Saksstatus::Avsluttet => domain::model::sak::Saksstatus::Avsluttet,
    }
}
