use anyhow::{Result, anyhow};

use domain::model::journalpost::{JournalpostType, Journalpoststatus};
use domain::model::tilgang::Tilgang;
use sikri_client::domain::journalpost_response::JournalpostRespons as SikriJournalpostResponse;

use crate::query::mapping::fra_sikri_til_domene::dokument::from_sikri_dokument_to_domain_dokument;

pub async fn from_sikri_journalpost_to_domain_journalpost(
    sikri_journalpost: SikriJournalpostResponse,
) -> Result<domain::model::journalpost::Journalpost> {
    let client_reference = None;

    let mut dokumenter = Vec::new();
    if let Some(docs) = sikri_journalpost.dokumenter_respons.clone() {
        for doc in docs {
            dokumenter.push(from_sikri_dokument_to_domain_dokument(doc).await?);
        }
    }

    let journalpost_response = domain::model::journalpost::Journalpost {
        client_reference,
        tittel: sikri_journalpost
            .clone()
            .tittel
            .ok_or_else(|| anyhow!("Journalpost har ikke tittel."))?,
        dokument_dato: sikri_journalpost
            .clone()
            .dokument_dato
            .ok_or_else(|| anyhow!("Journalpost har ikke dokument dato."))?,
        journalposttype: journalposttype_from_char(
            sikri_journalpost
                .clone()
                .journalposttype
                .ok_or_else(|| anyhow!("Journalpost har ikke journalposttype."))?
                .chars()
                .next()
                .ok_or_else(|| anyhow!("JournalpostType string har ingen chars."))?,
        )?,
        journalstatus: journalstatus_from_char(
            sikri_journalpost
                .journalstatus
                .clone()
                .ok_or_else(|| anyhow!("Journalpost har ikke journalstatus."))?
                .chars()
                .next()
                .ok_or_else(|| anyhow!("journalstatus string har ingen chars."))?,
        )?,
        tilgang: from_sikri_journalpost_to_domain_tilgang(sikri_journalpost.clone()),
        saksbehandler: sikri_journalpost
            .saksbehandler
            .ok_or_else(|| anyhow!("Journalpost har ikke saksbehandler."))?,
        dokumenter,
        journalpost_id: sikri_journalpost.journalpost_id,
        kildesystem: sikri_journalpost.kildesystem,
    };
    Ok(journalpost_response)
}

pub fn journalstatus_from_char(c: char) -> Result<Journalpoststatus> {
    let journalpoststatus = match c {
        'S' => Journalpoststatus::Registrert,
        'R' => Journalpoststatus::Reservert,
        'M' => Journalpoststatus::Midlertidig,
        'F' => Journalpoststatus::Ferdig,
        'E' => Journalpoststatus::Ekspedert,
        'J' => Journalpoststatus::Journalført,
        _ => {
            return Err(anyhow!("Ukjent Journalpoststatus: {c}"));
        }
    };
    Ok(journalpoststatus)
}

pub fn journalposttype_from_char(c: char) -> Result<JournalpostType> {
    let journalpost_type = match c {
        'I' => JournalpostType::Inngående,
        'U' => JournalpostType::Utgående,
        'X' => JournalpostType::InterntNotat,
        _ => {
            return Err(anyhow!("Ukjent JournalpostType: {c}"));
        }
    };
    Ok(journalpost_type)
}

fn from_sikri_journalpost_to_domain_tilgang(
    sikri_journalpost: SikriJournalpostResponse,
) -> Option<Tilgang> {
    sikri_journalpost
        .tilgangskode
        .zip(sikri_journalpost.tilgangshjemmel)
        .map(|(tilgangskode, tilgangshjemmel)| Tilgang {
            tilgangskode,
            tilgangshjemmel,
        })
}
