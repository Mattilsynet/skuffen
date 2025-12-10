use anyhow::Result;
use lib_schemas::skuffen::sak::{
    SakKey as DtoSakKey, SakResponse as DtoSak, Saksnummer as DtoSaksnummer,
    Saksstatus as DtoSaksstaus,
};

use crate::mapping::fra_domene_til_dto::journalpost::from_domain_journalpost_to_dto;

pub fn from_domain_sak_to_dto(sak: domain::model::sak::Sak) -> Result<DtoSak> {
    Ok(DtoSak {
        sakstittel: sak.sakstittel,
        saksbehandler: sak.saksbehandler,
        saksstatus: from_domain_saksstatus_to_dto(sak.saksstatus),
        unntatt_offentlighet: sak.unntatt_offentlighet,
        saksnr: from_domain_saksnummer_to_dto(sak.saksnr)?, //TODO: Hvordan også gi tilbake
        //SkuffenId?
        lukket: sak.lukket,
        kildesystem: sak.kildesystem,
        journalposter: Some(
            sak.journalposter
                .into_iter()
                .map(|jp| from_domain_journalpost_to_dto(jp.clone()))
                .collect::<Result<_>>()?,
        ),
    })
}

pub fn from_domain_sak_key_to_dto(key: domain::model::sak::SakKey) -> Result<DtoSakKey> {
    Ok(match key {
        domain::model::sak::SakKey::SkuffenId(uuid) => DtoSakKey::SkuffenId(uuid),
        domain::model::sak::SakKey::ArkivId(saksnr) => {
            DtoSakKey::ArkivId(from_domain_saksnummer_to_dto(saksnr)?)
        }
    })
}

fn from_domain_saksnummer_to_dto(
    saksnummer: domain::model::sak::Saksnummer,
) -> Result<DtoSaksnummer> {
    let dto_saksnummer = DtoSaksnummer::new(saksnummer.as_str())?;
    Ok(dto_saksnummer)
}

fn from_domain_saksstatus_to_dto(domain_saksstaus: domain::model::sak::Saksstatus) -> DtoSaksstaus {
    match domain_saksstaus {
        domain::model::sak::Saksstatus::UnderBehandling => DtoSaksstaus::UnderBehandling,
        domain::model::sak::Saksstatus::Ferdig => DtoSaksstaus::Ferdig,
        domain::model::sak::Saksstatus::Avsluttet => DtoSaksstaus::Avsluttet,
    }
}
