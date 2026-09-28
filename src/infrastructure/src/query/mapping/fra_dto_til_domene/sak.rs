use anyhow::Result;
use lib_schemas::skuffen::{
    query::queries::SakKey,
    sak::{Saksnummer, Saksstatus},
};

pub fn from_dto_sak_key_to_domain(dto_sak_key: SakKey) -> Result<domain::model::sak::SakKey> {
    Ok(match dto_sak_key {
        SakKey::ClientReference(client_reference) => {
            domain::model::sak::SakKey::ClientReference(client_reference)
        }
        SakKey::ArkivId(saksnummer) => {
            domain::model::sak::SakKey::ArkivId(from_dto_saksnumer_to_domain(saksnummer)?)
        }
    })
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
