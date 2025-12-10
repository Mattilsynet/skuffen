use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ElementsAvsenderMottaker {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forsendelsesmetode: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub er_mottaker: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub kopi: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub unntatt_offentlighet: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub person: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub til_saksbehandler: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub til_saksbehandler_enhet: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub navn: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub organisasjonsnummer: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub epost: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub telefon: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub postadresse: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub postnummer: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub poststed: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub utlandsadresse: Option<String>,
}
