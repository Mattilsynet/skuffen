use crate::dto::elements_journalpost::ElementsJournalpostRespons;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ElementsSakMedJournalposterResponse {
    pub sakstittel: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arkivdel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalenhet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksbehandler: Option<String>,
    #[serde(rename = "saksbehandlerEnhet", skip_serializing_if = "Option::is_none")]
    pub saksbehandler_enhet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksstatus: Option<String>,
    pub ordningsverdi: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tilgangskode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tilgangshjemmel: Option<String>,
    #[serde(rename = "virksomhetsmappeId", skip_serializing_if = "Option::is_none")]
    pub virksomhetsmappe_id: Option<String>,
    pub saksid: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksnr: Option<String>,
    #[serde(rename = "saksUrl", skip_serializing_if = "Option::is_none")]
    pub saks_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kildesystem: Option<String>,
    pub lukket: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mappetype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub antall_journalposter: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalposter: Option<Vec<ElementsJournalpostRespons>>,
}
