use anyhow::{Context, Result, anyhow};
use chrono::NaiveDateTime;

use domain::model::journalpost::{JournalpostType, Journalpoststatus};
use domain::model::tilgang::{Tilgang, Tilgangshjemmel, Tilgangskode};
use sikri_client::domain::journalpost_response::JournalpostRespons as SikriJournalpostResponse;

use crate::query::mapping::fra_sikri_til_domene::dokument::from_sikri_dokumenter_to_domain;

pub fn from_sikri_journalpost_to_domain_journalpost(
    sikri_journalpost: SikriJournalpostResponse,
) -> Result<domain::model::journalpost::Journalpost> {
    let tilgang = from_sikri_journalpost_to_domain_tilgang(&sikri_journalpost)?;
    let dokumenter = from_sikri_dokumenter_to_domain(
        sikri_journalpost.dokumenter_respons.unwrap_or_default(),
        sikri_journalpost.hoveddok_id,
        sikri_journalpost.har_hoveddokument,
    )?;

    Ok(domain::model::journalpost::Journalpost {
        client_reference: None,
        tittel: sikri_journalpost
            .tittel
            .ok_or_else(|| anyhow!("Journalpost har ikke tittel."))?,
        dokument_dato: dokument_dato_from_sikri(sikri_journalpost.dokument_dato.as_deref())?,
        journalposttype: journalposttype_from_char(
            forste_tegn(sikri_journalpost.journalposttype.as_deref())
                .ok_or_else(|| anyhow!("Journalpost har ikke journalposttype."))?,
        )?,
        journalstatus: journalstatus_from_char(
            forste_tegn(sikri_journalpost.journalstatus.as_deref())
                .ok_or_else(|| anyhow!("Journalpost har ikke journalstatus."))?,
        )?,
        tilgang,
        saksbehandler: sikri_journalpost.saksbehandler,
        dokumenter,
        journalpost_id: sikri_journalpost.journalpost_id,
        kildesystem: sikri_journalpost.kildesystem,
    })
}

/// Arkivets dokumentdato er dato og klokkeslett uten tidssone, og bevares slik.
fn dokument_dato_from_sikri(dokument_dato: Option<&str>) -> Result<NaiveDateTime> {
    let dokument_dato =
        dokument_dato.ok_or_else(|| anyhow!("Journalpost har ikke dokument dato."))?;
    dokument_dato
        .parse::<NaiveDateTime>()
        .context("Journalpostens dokumentdato er ikke dato og klokkeslett uten tidssone.")
}

fn forste_tegn(verdi: Option<&str>) -> Option<char> {
    verdi.and_then(|v| v.chars().next())
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
    sikri_journalpost: &SikriJournalpostResponse,
) -> Result<Option<Tilgang>> {
    // Fail-closed: delvis tilgang (kun kode eller kun hjemmel) må aldri se
    // uskjermet ut. Da avviser vi i stedet for å returnere None.
    match (
        sikri_journalpost.tilgangskode.clone(),
        sikri_journalpost.tilgangshjemmel.clone(),
    ) {
        (Some(tilgangskode), Some(tilgangshjemmel)) => Ok(Some(Tilgang {
            tilgangskode: Tilgangskode::new(tilgangskode)?,
            tilgangshjemmel: Tilgangshjemmel::new(tilgangshjemmel)?,
        })),
        (None, None) => Ok(None),
        (Some(_), None) | (None, Some(_)) => Err(anyhow!(
            "Sikri journalpost har delvis tilgang (kun kode eller kun hjemmel); avviser fail-closed"
        )),
    }
}
