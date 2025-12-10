use anyhow::Result;
use lib_schemas::skuffen::sak::{SakKey, Saksnummer, Saksstatus};

// pub fn from_dto_sak_to_domain(dto_sak: ) -> Result<domain::model::sak::Sak> {
//     let domain_sak = domain::model::sak::Sak {
//         sakstittel: dto_sak.sakstittel,
//         saksbehandler: dto_sak.saksbehandler,
//         saksstatus: from_dto_sakstatus_to_domain(dto_sak.saksstatus),
//         unntatt_offentlighet: dto_sak.unntatt_offentlighet,
//         saksnr: from_dto_saksnumer_to_domain(dto_sak.saksnr)?,
//         kildesystem: dto_sak.kildesystem,
//         lukket: dto_sak.lukket,
//         journalposter: dto_sak.journalposter.unwrap_or_else(|| Vec::new()).iter().map(f),
//     }
// }

pub fn from_dto_sak_key_to_domain(dto_sak_key: SakKey) -> Result<domain::model::sak::SakKey> {
    let key = match dto_sak_key {
        SakKey::SkuffenId(uuid) => domain::model::sak::SakKey::SkuffenId(uuid),
        SakKey::ArkivId(saksnummer) => {
            domain::model::sak::SakKey::ArkivId(from_dto_saksnumer_to_domain(saksnummer)?)
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
