use crate::dto::elements_avsender_mottaker::ElementsAvsenderMottaker;
use crate::dto::elements_dokument::ElementsDokument;
use crate::dto::elements_dokument_response::ElementsDokumentRespons;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ElementsJournalpostRespons {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksbehandler_enhet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avsendere_mottakere: Option<Vec<ElementsAvsenderMottaker>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dokumenter: Option<Vec<ElementsDokument>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalpost_id: Option<i32>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub antall_vedlegg: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dokument_dato: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoveddok_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoveddokument_filtype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub har_hoveddokument: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoveddokument_tittel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dokumenter_respons: Option<Vec<ElementsDokumentRespons>>,
}
