use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ElementsDokumentRespons {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dokument_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoveddok_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dokument_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hoveddokument: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisert: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tittel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filtype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dokument_base64: Option<String>,
}
