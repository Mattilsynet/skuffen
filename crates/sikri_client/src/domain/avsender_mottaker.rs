use crate::dto::elements_avsender_mottaker::ElementsAvsenderMottaker;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AvsenderMottaker {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub navn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adresse: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub postnummer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poststed: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub land: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_type: Option<String>,
}

impl From<ElementsAvsenderMottaker> for AvsenderMottaker {
    fn from(src: ElementsAvsenderMottaker) -> Self {
        AvsenderMottaker {
            navn: src.navn,

            adresse: src.postadresse,

            postnummer: src.postnummer,
            poststed: src.poststed,

            land: None, //sikri json respons har ikke land

            id: src.id.map(|id| id.to_string()),

            id_type: None, //sikri json response har ikke id type
        }
    }
}
