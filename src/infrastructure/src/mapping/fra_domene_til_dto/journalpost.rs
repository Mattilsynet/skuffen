use anyhow::Result;
use lib_schemas::skuffen::{
    journalpost::{JournalpostResponse, JournalpostType, Journalpoststatus},
    tilgang::Tilgang,
};

use crate::mapping::fra_domene_til_dto::dokument::from_domain_dokument_to_dto;

pub fn from_domain_journalpost_to_dto(
    domain_journalpost: domain::model::journalpost::Journalpost,
) -> Result<JournalpostResponse> {
    let journalpost_response = JournalpostResponse {
        tittel: domain_journalpost.tittel,
        dokument_dato: domain_journalpost.dokument_dato,
        journalposttype: from_domain_journalposttype_to_dto(domain_journalpost.journalposttype),
        journalstatus: from_domain_journalpoststatus_to_dto(domain_journalpost.journalstatus),
        tilgang: from_domain_tilgang_to_dto(domain_journalpost.tilgang),
        saksbehandler: domain_journalpost.saksbehandler,
        dokumenter: domain_journalpost
            .dokumenter
            .iter()
            .map(|doc| from_domain_dokument_to_dto(doc.clone()))
            .collect(),
        journalpost_id: domain_journalpost.journalpost_id,
        kildesystem: domain_journalpost.kildesystem.unwrap_or_default(),
    };
    Ok(journalpost_response)
}

fn from_domain_journalposttype_to_dto(
    domain_journalposttype: domain::model::journalpost::JournalpostType,
) -> JournalpostType {
    match domain_journalposttype {
        domain::model::journalpost::JournalpostType::Inngående => JournalpostType::Inngående,
        domain::model::journalpost::JournalpostType::Utgående => JournalpostType::Utgående,
        domain::model::journalpost::JournalpostType::InterntNotat => JournalpostType::InterntNotat,
    }
}

fn from_domain_journalpoststatus_to_dto(
    domain_journalpoststatus: domain::model::journalpost::Journalpoststatus,
) -> Journalpoststatus {
    match domain_journalpoststatus {
        domain::model::journalpost::Journalpoststatus::Registrert => Journalpoststatus::Registrert,
        domain::model::journalpost::Journalpoststatus::Reservert => Journalpoststatus::Reservert,
        domain::model::journalpost::Journalpoststatus::Midlertidig => {
            Journalpoststatus::Midlertidig
        }
        domain::model::journalpost::Journalpoststatus::Ferdig => Journalpoststatus::Ferdig,
        domain::model::journalpost::Journalpoststatus::Ekspedert => Journalpoststatus::Ekspedert,
        domain::model::journalpost::Journalpoststatus::Journalført => {
            Journalpoststatus::Journalført
        }
    }
}

fn from_domain_tilgang_to_dto(
    domain_tilgang: Option<domain::model::tilgang::Tilgang>,
) -> Option<Tilgang> {
    domain_tilgang.map(|t| Tilgang {
        tilgangskode: t.tilgangskode,
        tilgangshjemmel: t.tilgangshjemmel,
    })
}
