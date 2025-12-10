use anyhow::{anyhow, Result};
use domain::model::dokument::Dokument;
use domain::model::journalpost::{JournalpostType, Journalpoststatus};
use sikri_client::domain::journalpost_response::JournalpostRespons as SikriJournalpostResponse;

use crate::mapping::fra_sikri_til_domene::dokument::from_sikri_dokument_to_domain_dokument;

pub fn from_sikri_journalpost_to_domain_journalpost(
    sikri_journalpost: SikriJournalpostResponse,
) -> Result<domain::model::journalpost::Journalpost> {
    let journalpost_response = domain::model::journalpost::Journalpost {
        tittel: sikri_journalpost
            .tittel
            .ok_or_else(|| anyhow!("Journalpost har ikke tittel."))?,
        dokument_dato: sikri_journalpost
            .dokument_dato
            .ok_or_else(|| anyhow!("Journalpost har ikke dokument dato."))?,
        journalposttype: journalposttype_from_char(
            sikri_journalpost
                .journalposttype
                .ok_or_else(|| anyhow!("Journalpost har ikke journalposttype."))?
                .chars()
                .next()
                .ok_or_else(|| anyhow!("JournalpostType string har ingen chars."))?,
        )?,
        journalstatus: journalstatus_from_char(
            sikri_journalpost
                .journalstatus
                .ok_or_else(|| anyhow!("Journalpost har ikke journalstatus."))?
                .chars()
                .next()
                .ok_or_else(|| anyhow!("journalstatus string har ingen chars."))?,
        )?,
        unntatt_offentlighet: true, //TODO: Satt til true for å teste. Fix!
        saksbehandler: sikri_journalpost
            .saksbehandler
            .ok_or_else(|| anyhow!("Journalpost har ikke saksbehandler."))?,
        dokumenter: sikri_journalpost
            .dokumenter_respons
            .unwrap_or_else(|| Vec::new())
            .iter()
            .map(|doc| from_sikri_dokument_to_domain_dokument(doc.clone()))
            .collect::<anyhow::Result<Vec<Dokument>>>()?,
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
