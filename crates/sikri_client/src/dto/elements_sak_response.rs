use crate::dto::elements_journalpost::ElementsJournalpostRespons;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ElementsSakMedJournalposterResponse {
    pub sakstittel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arkivdel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalenhet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksbehandler: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksbehandler_enhet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksstatus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordningsverdi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tilgangskode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tilgangshjemmel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virksomhetsmappe_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksid: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksnr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saks_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kildesystem: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lukket: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mappetype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub antall_journalposter: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalposter: Option<Vec<ElementsJournalpostRespons>>,
}
