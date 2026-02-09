use serde::{Deserialize, Serialize};
use std::fmt::Display;


#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Arkivdel {
    // Tilsynsdivisjonene må bli mappet om til SAK og 
    // Hovedkontoret må bli mappet om til SAKHK 
    Tilsynsdivisjonene,
    Hovedkontoret,
}

impl Display for Arkivdel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Arkivdel::Tilsynsdivisjonene => write!(f, "SAK"),
            Arkivdel::Hovedkontoret => write!(f, "SAKHK"),
        }
    }
}



#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tilgang {
    pub tilgangskode: String,
    pub tilgangshjemmel: String,
}




#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NySak {
    /// Sakstittel (required)
    /// Max length: 256, Min length: 1
    pub sakstittel: String,

    /// Arkivdel som saken skal opprettes i. Eks: MATS
    pub arkivdel: Arkivdel  ,

    /// Saksbehandler
    pub saksbehandler_id: String,
    pub saksbehandler_enhet: String,

    /// Ordningsverdi slik den er registrert i Mattilsynets arkivnøkkel (required)
    /// Min length: 1
    pub ordningsverdi: String,

    /// Tilgangskode fra kodeverk TILGANGSKODE
    /// Hjemmel for at saken skal være unntatt fra offentligheten.
    /// Kode fra kodeverk TILGANGSHJEMMEL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tilgang: Option<Tilgang>,

    /// VirksomhetsmappeId kommer fra saksbehandling i MATS.
    /// Dersom denne er inkludert, vil den opprettede saken knyttes til virksomheten via tilleggsattributt1 på saken.
    /// NB! Flere saker kan være knyttet mot samme VirksomhetsmappeId.
    #[serde(rename = "virksomhetsmappeId", skip_serializing_if = "Option::is_none")]
    pub virksomhetsmappe_id: Option<String>,
}
