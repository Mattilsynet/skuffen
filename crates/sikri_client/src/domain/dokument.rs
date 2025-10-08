use crate::dto::elements_dokument::ElementsDokument;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Dokument {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tittel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filtype: Option<String>,
    // Base64
    #[serde(skip_serializing_if = "Option::is_none")]
    pub innhold: Option<String>,
}

impl From<ElementsDokument> for Dokument {
    fn from(src: ElementsDokument) -> Self {
        Dokument {
            tittel: src.tittel,
            filtype: src.filtype,
            innhold: src.innhold,
        }
    }
}
