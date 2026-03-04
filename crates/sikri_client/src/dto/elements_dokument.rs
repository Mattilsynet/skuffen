use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ElementsDokument {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tittel: Option<String>,
    pub hoveddokument: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filtype: Option<String>,
    // Base64
    #[serde(
        rename = "dokumentBase64",
        alias = "filInnhold",
        alias = "innhold",
        skip_serializing_if = "Option::is_none"
    )]
    pub innhold: Option<String>,
}
