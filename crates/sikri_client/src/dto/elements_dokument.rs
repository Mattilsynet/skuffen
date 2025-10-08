use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ElementsDokument {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tittel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filtype: Option<String>,
    // Base64
    #[serde(skip_serializing_if = "Option::is_none")]
    pub innhold: Option<String>,
}
