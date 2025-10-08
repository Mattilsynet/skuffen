use crate::dto::elements_dokument_response::ElementsDokumentRespons;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DokumentRespons {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dokument_id: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tittel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filtype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl From<ElementsDokumentRespons> for DokumentRespons {
    fn from(src: ElementsDokumentRespons) -> Self {
        DokumentRespons {
            dokument_id: src.dokument_id,
            tittel: src.tittel,
            filtype: src.filtype,
            url: src.url,
        }
    }
}
