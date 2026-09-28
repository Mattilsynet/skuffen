use anyhow::Result;
use lib_schemas::skuffen::query::responses::SakResponse as DtoSak;
use lib_schemas::skuffen::sak::{
    Ordningsverdi as DtoOrdningsverdi, Saksnummer as DtoSaksnummer, Saksstatus as DtoSaksstaus,
    Sakstittel as DtoSakstittel,
};

use crate::query::mapping::fra_domene_til_dto::journalpost::from_domain_journalpost_to_dto;
use crate::query::mapping::fra_domene_til_dto::tilgang::from_domain_tilgang_to_tilgjengelighet;

pub fn from_domain_sak_to_dto(sak: domain::model::sak::Sak) -> Result<DtoSak> {
    Ok(DtoSak {
        sakstittel: DtoSakstittel::try_from(sak.sakstittel.0.as_str())?,
        saksbehandler: Some(sak.saksbehandler),
        saksbehandler_enhet: None,
        saksstatus: from_domain_saksstatus_to_dto(sak.saksstatus),
        tilgjengelighet: from_domain_tilgang_to_tilgjengelighet(sak.tilgang),
        ordningsverdi: from_domain_ordningsverdi_to_dto(sak.ordningsverdi)?,
        saksnummer: from_domain_saksnummer_to_dto(sak.saksnummer)?,
        kildesystem: sak.kildesystem,
        lukket: sak.lukket,
        journalposter: sak
            .journalposter
            .map(|journalposter| {
                journalposter
                    .into_iter()
                    .map(from_domain_journalpost_to_dto)
                    .collect::<Result<_>>()
            })
            .transpose()?,
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

fn from_domain_ordningsverdi_to_dto(
    domain_ordningsverdi: domain::model::sak::Ordningsverdi,
) -> Result<DtoOrdningsverdi> {
    let ov = DtoOrdningsverdi::new(domain_ordningsverdi.get().to_string())?;
    Ok(ov)
}
