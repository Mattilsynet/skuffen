use anyhow::Result;
use lib_schemas::skuffen::{
    sak::{
        Ordningsverdi as DtoOrdningsverdi, SakKeyResponse as DtoSakKey, SakResponse as DtoSak,
        Saksnummer as DtoSaksnummer, Saksstatus as DtoSaksstaus,
    },
    tilgang::Tilgang as DtoTilgang,
};

use crate::mapping::{
    fra_domene_til_dto::journalpost::from_domain_journalpost_to_dto,
    lookup::key_mapping_queries::lookup_arkiv_id_fra_skuffen_id,
};

pub async fn from_domain_sak_to_dto(sak: domain::model::sak::Sak) -> Result<DtoSak> {
    Ok(DtoSak {
        sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(sak.sakstittel.0.as_str())?,
        saksbehandler: sak.saksbehandler,
        saksstatus: from_domain_saksstatus_to_dto(sak.saksstatus),
        tilgang: from_domain_tilgang_to_dto(sak.tilgang),
        sak_key: from_domain_sak_key_to_dto(sak.sak_key).await?,
        lukket: sak.lukket,
        kildesystem: sak.kildesystem,
        journalposter: Some(
            sak.journalposter
                .into_iter()
                .map(|jp| from_domain_journalpost_to_dto(jp.clone()))
                .collect::<Result<_>>()?,
        ),
        ordningsverdi: from_domain_ordningsverdi_to_dto(sak.ordningsverdi)?,
    })
}

pub async fn from_domain_sak_key_to_dto(key: domain::model::sak::SakKey) -> Result<DtoSakKey> {
    Ok(DtoSakKey {
        skuffen_id: key.skuffen_id,
        arkiv_id: Some(from_domain_saksnummer_to_dto(
            lookup_arkiv_id_fra_skuffen_id(key.skuffen_id).await?,
        )?),
    })
}

fn from_domain_saksnummer_to_dto(
    saksnummer: domain::model::sak::Saksnummer,
) -> Result<DtoSaksnummer> {
    let dto_saksnummer = DtoSaksnummer::new(saksnummer.as_str())?;
    Ok(dto_saksnummer)
}

fn from_domain_tilgang_to_dto(
    tilgang: Option<domain::model::tilgang::Tilgang>,
) -> Option<DtoTilgang> {
    tilgang.map(|t| DtoTilgang {
        tilgangskode: t.tilgangskode,
        tilgangshjemmel: t.tilgangshjemmel,
    })
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
