use crate::domain::ny_sak::{NySak, Tilgang};
use serde::{Deserialize, Serialize};

pub const JOURNALENHET: &str = "DOKSENTER";

pub const DEFAULT_SAKSSTATUS: &str = "B";


/// Represents a Sak (case) in the Sikri API.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ElementsSak {
    /// Sakstittel (required)
    /// Max length: 256, Min length: 1
    pub sakstittel: String,

    /// Arkivdel som saken skal opprettes i. 
    pub arkivdel: String,

    /// Journalenhet som saken skal opprettes i. Eks: DOKSENTER
    pub journalenhet: String,

    /// Saksbehandler
    pub saksbehandler: String,

    /// SaksbehandlerEnhet
    pub saksbehandler_enhet: String,

    /// Saksstatus
    pub saksstatus: String,

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


impl From<NySak> for ElementsSak {
    fn from(src: NySak) -> Self {
        ElementsSak {
            sakstittel: src.sakstittel,
            arkivdel: src.arkivdel.to_string(),
            journalenhet: JOURNALENHET.to_string(),
            saksbehandler: src.saksbehandler_id,
            saksbehandler_enhet: src.saksbehandler_enhet,
            saksstatus: DEFAULT_SAKSSTATUS.to_string(),
            ordningsverdi: src.ordningsverdi,
            tilgang: src.tilgang,
            virksomhetsmappe_id: src.virksomhetsmappe_id,
        }
    }
}

impl ElementsSak {
    /// Validates required fields and length constraints.
    pub fn validate(&self) -> anyhow::Result<(), String> {
        // Validate sakstittel
        let len = self.sakstittel.len();
        if len == 0 || len > 256 {
            return Err(format!(
                "sakstittel must be between 1 and 256 characters, got length {len}"
            ));
        }

        // Validate ordningsverdi
        if self.ordningsverdi.trim().is_empty() {
            return Err("ordningsverdi must be at least 1 character".to_string());
        }

        Ok(())
    }
}
