use crate::domain::avsender_mottaker::AvsenderMottaker;
use crate::domain::dokument::Dokument;
use crate::dto::elements_journalpost::ElementsJournalpostRespons;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Journalpost {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tittel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalposttype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalstatus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avskriv_direkte: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avskrivningsmaate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tilgangskode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tilgangshjemmel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksbehandler: Option<String>,
    #[serde(rename = "saksbehandlerEnhet", skip_serializing_if = "Option::is_none")]
    pub saksbehandler_enhet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avsendere_mottakere: Option<Vec<AvsenderMottaker>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dokumenter: Option<Vec<Dokument>>,
    pub journalpost_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalpostnr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalpost_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kildesystem: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lopenr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dokumentnummer: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dokumentkategori: Option<String>,
    pub antall_vedlegg: i32,
    #[serde(rename = "dokumentDato", skip_serializing_if = "Option::is_none")]
    pub dokument_dato: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoveddok_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoveddokument_filtype: Option<String>,
    pub har_hoveddokument: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoveddokument_tittel: Option<String>,
}

impl From<ElementsJournalpostRespons> for Journalpost {
    fn from(src: ElementsJournalpostRespons) -> Self {
        Journalpost {
            tittel: src.tittel,
            journalposttype: src.journalposttype,
            journalstatus: src.journalstatus,
            avskriv_direkte: src.avskriv_direkte,
            avskrivningsmaate: src.avskrivningsmaate,
            tilgangskode: src.tilgangskode,
            tilgangshjemmel: src.tilgangshjemmel,
            saksbehandler: src.saksbehandler,
            saksbehandler_enhet: src.saksbehandler_enhet,
            avsendere_mottakere: src
                .avsendere_mottakere
                .map(|v| v.into_iter().map(Into::into).collect()),
            dokumenter: src
                .dokumenter
                .map(|v| v.into_iter().map(Into::into).collect()),
            journalpost_id: src.journalpost_id.unwrap_or_default(),
            journalpostnr: src.journalpostnr,
            journalpost_url: src.journalpost_url,
            kildesystem: src.kildesystem,
            lopenr: src.lopenr,
            dokumentnummer: src.dokumentnummer,
            dokumentkategori: src.dokumentkategori,
            antall_vedlegg: src.antall_vedlegg.unwrap_or_default(),
            dokument_dato: src.dokument_dato,
            hoveddok_id: src.hoveddok_id,
            hoveddokument_filtype: src.hoveddokument_filtype,
            har_hoveddokument: src.har_hoveddokument.unwrap_or_default(),
            hoveddokument_tittel: src.hoveddokument_tittel,
        }
    }
}
