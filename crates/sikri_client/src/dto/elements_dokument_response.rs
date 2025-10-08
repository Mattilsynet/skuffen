use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ElementsDokumentRespons {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dokument_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tittel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filtype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}
