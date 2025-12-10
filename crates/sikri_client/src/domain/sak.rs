use crate::domain::journalpost_response::JournalpostRespons;
use crate::dto::elements_sak_response::ElementsSakMedJournalposterResponse;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SakRespons {
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
    pub journalposter: Option<Vec<JournalpostRespons>>,
}

impl From<ElementsSakMedJournalposterResponse> for SakRespons {
    fn from(src: ElementsSakMedJournalposterResponse) -> Self {
        SakRespons {
            sakstittel: src.sakstittel,
            arkivdel: src.arkivdel,
            journalenhet: src.journalenhet,
            saksbehandler: src.saksbehandler,
            saksbehandler_enhet: src.saksbehandler_enhet,
            saksstatus: src.saksstatus,
            ordningsverdi: src.ordningsverdi.unwrap_or_default(),
            tilgangskode: src.tilgangskode,
            tilgangshjemmel: src.tilgangshjemmel,
            virksomhetsmappe_id: src.virksomhetsmappe_id,
            saksid: src.saksid.unwrap_or_default(),
            saksnr: src.saksnr,
            saks_url: src.saks_url,
            kildesystem: src.kildesystem,
            lukket: src.lukket.unwrap_or_default(),
            mappetype: src.mappetype,
            antall_journalposter: src.antall_journalposter,
            journalposter: src.journalposter.map(|v| {
                v.into_iter()
                    .map(JournalpostRespons::from)
                    .collect::<Vec<JournalpostRespons>>()
            }),
        }
    }
}
