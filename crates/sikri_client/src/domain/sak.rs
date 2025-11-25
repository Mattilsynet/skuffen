use crate::dto::elements_sak_response::ElementsSakMedJournalposterResponse;
use lib_schemas::arkiv::v2::sak::Saksnummer;
use serde::{Deserialize, Serialize};

// #[derive(Debug, Serialize, Deserialize, Clone)]
// pub struct SakMedJournalposterResponse {
//     pub sakstittel: String,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub arkivdel: Option<String>,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub journalenhet: Option<String>,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub saksbehandler: Option<String>,
//     #[serde(rename = "saksbehandlerEnhet", skip_serializing_if = "Option::is_none")]
//     pub saksbehandler_enhet: Option<String>,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub saksstatus: Option<String>,
//     pub ordningsverdi: String,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub tilgangskode: Option<String>,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub tilgangshjemmel: Option<String>,
//     #[serde(rename = "virksomhetsmappeId", skip_serializing_if = "Option::is_none")]
//     pub virksomhetsmappe_id: Option<String>,
//     pub saksid: i32,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub saksnr: Option<String>,
//     #[serde(rename = "saksUrl", skip_serializing_if = "Option::is_none")]
//     pub saks_url: Option<String>,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub kildesystem: Option<String>,
//     pub lukket: bool,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub mappetype: Option<String>,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub antall_journalposter: Option<i32>,
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub journalposter: Option<Vec<crate::domain::journalpost::Journalpost>>,
// }

// impl From<ElementsSakMedJournalposterResponse> for SakMedJournalposterResponse {
//     fn from(value: ElementsSakMedJournalposterResponse) -> Self {
//         SakMedJournalposterResponse {
//             sakstittel: value.sakstittel,
//             arkivdel: value.arkivdel,
//             journalenhet: value.journalenhet,
//             saksbehandler: value.saksbehandler,
//             saksbehandler_enhet: value.saksbehandler_enhet,
//             saksstatus: value.saksstatus,
//             ordningsverdi: value.ordningsverdi,
//             tilgangskode: value.tilgangskode,
//             tilgangshjemmel: value.tilgangshjemmel,
//             virksomhetsmappe_id: value.virksomhetsmappe_id,
//             saksid: value.saksid,
//             saksnr: value.saksnr,
//             saks_url: value.saks_url,
//             kildesystem: value.kildesystem,
//             lukket: value.lukket,
//             mappetype: value.mappetype,
//             antall_journalposter: value.antall_journalposter,
//             journalposter: value.journalposter,
//         }
//     }
// }

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SakResponse {
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
}

impl From<ElementsSakMedJournalposterResponse> for SakResponse {
    fn from(value: ElementsSakMedJournalposterResponse) -> Self {
        SakResponse {
            sakstittel: value.sakstittel,
            arkivdel: value.arkivdel,
            journalenhet: value.journalenhet,
            saksbehandler: value.saksbehandler,
            saksbehandler_enhet: value.saksbehandler_enhet,
            saksstatus: value.saksstatus,
            ordningsverdi: value.ordningsverdi,
            tilgangskode: value.tilgangskode,
            tilgangshjemmel: value.tilgangshjemmel,
            virksomhetsmappe_id: value.virksomhetsmappe_id,
            saksid: value.saksid,
            saksnr: value.saksnr,
            saks_url: value.saks_url,
            kildesystem: value.kildesystem,
            lukket: value.lukket,
            mappetype: value.mappetype,
        }
    }
}

impl From<SakResponse> for lib_schemas::arkiv::v2::sak::SakResponse {
    fn from(value: SakResponse) -> Self {
        lib_schemas::arkiv::v2::sak::SakResponse {
            sakstittel: value.sakstittel,
            saksbehandler: value.saksbehandler.unwrap_or_default(),
            saksstatus: value.saksstatus.unwrap_or_default(),
            unntatt_offentlighet: true, //TODO: Satt til true bare for å teste. FIX
            saksnr: Saksnummer::new(value.saksnr.unwrap_or_default()).unwrap(), //TODO: Fix unwrap
            lukket: value.lukket,
        }
    }
}
