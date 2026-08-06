use anyhow::Result;
use lib_schemas::skuffen::journalpost::{JournalpostId, JournalpostType, Journalpoststatus};
use lib_schemas::skuffen::query::responses::JournalpostResponse;

use crate::query::mapping::fra_domene_til_dto::dokument::from_domain_dokument_to_dto;
use crate::query::mapping::fra_domene_til_dto::tilgang::from_domain_tilgang_to_tilgjengelighet;

pub fn from_domain_journalpost_to_dto(
    domain_journalpost: domain::model::journalpost::Journalpost,
) -> Result<JournalpostResponse> {
    let journalpost_response = JournalpostResponse {
        tittel: domain_journalpost.tittel,
        dokument_dato: domain_journalpost.dokument_dato,
        journalposttype: from_domain_journalposttype_to_dto(domain_journalpost.journalposttype),
        journalstatus: from_domain_journalpoststatus_to_dto(domain_journalpost.journalstatus),
        tilgjengelighet: from_domain_tilgang_to_tilgjengelighet(domain_journalpost.tilgang),
        saksbehandler: Some(domain_journalpost.saksbehandler),
        saksbehandler_enhet: None,
        // Domenets read-modell bærer ennå ikke avsender/mottaker; feltet
        // rapporteres derfor som fraværende (None), ikke tom liste.
        korrespondanseparter: None,
        dokumenter: domain_journalpost
            .dokumenter
            .iter()
            .map(|doc| from_domain_dokument_to_dto(doc.clone()))
            .collect(),
        journalpost_id: JournalpostId(domain_journalpost.journalpost_id.to_string()),
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
