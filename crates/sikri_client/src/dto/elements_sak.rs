use crate::domain::ny_sak::NySak;
use crate::dto::elements_sak_response::ElementsSakMedJournalposterResponse;
use serde::{Deserialize, Serialize};

/// Represents a Sak (case) in the Sikri API.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ElementsSak {
    /// Sakstittel (required)
    /// Max length: 256, Min length: 1
    pub sakstittel: String,

    /// Arkivdel som saken skal opprettes i. Eks: MATS
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arkivdel: Option<String>,

    /// Journalenhet som saken skal opprettes i. Eks: DOKSENTER
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalenhet: Option<String>,

    /// Saksbehandler
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksbehandler: Option<String>,

    /// SaksbehandlerEnhet
    #[serde(rename = "saksbehandlerEnhet", skip_serializing_if = "Option::is_none")]
    pub saksbehandler_enhet: Option<String>,

    /// Saksstatus
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksstatus: Option<String>,

    /// Ordningsverdi slik den er registrert i Mattilsynets arkivnøkkel (required)
    /// Min length: 1
    pub ordningsverdi: String,

    /// Tilgangskode fra kodeverk TILGANGSKODE
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tilgangskode: Option<String>,

    /// Hjemmel for at saken skal være unntatt fra offentligheten.
    /// Kode fra kodeverk TILGANGSHJEMMEL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tilgangshjemmel: Option<String>,

    /// VirksomhetsmappeId kommer fra saksbehandling i MATS.
    /// Dersom denne er inkludert, vil den opprettede saken knyttes til virksomheten via tilleggsattributt1 på saken.
    /// NB! Flere saker kan være knyttet mot samme VirksomhetsmappeId.
    #[serde(rename = "virksomhetsmappeId", skip_serializing_if = "Option::is_none")]
    pub virksomhetsmappe_id: Option<String>,
}

impl From<ElementsSakMedJournalposterResponse> for ElementsSak {
    fn from(value: ElementsSakMedJournalposterResponse) -> Self {
        ElementsSak {
            sakstittel: value.sakstittel,
            arkivdel: value.arkivdel,
            journalenhet: value.journalenhet,
            saksbehandler: value.saksbehandler,
            saksbehandler_enhet: value.saksbehandler_enhet,
            saksstatus: value.saksstatus,
            ordningsverdi: value.ordningsverdi,
            tilgangskode: value.tilgangskode,
            tilgangshjemmel: value.tilgangshjemmel,
            virksomhetsmappe_id: value.virksomhetsmappe_id,
        }
    }
}

impl From<NySak> for ElementsSak {
    fn from(src: NySak) -> Self {
        ElementsSak {
            sakstittel: src.sakstittel,
            arkivdel: src.arkivdel,
            journalenhet: src.journalenhet,
            saksbehandler: src.saksbehandler,
            saksbehandler_enhet: src.saksbehandler_enhet,
            saksstatus: src.saksstatus,
            ordningsverdi: src.ordningsverdi,
            tilgangskode: src.tilgangskode,
            tilgangshjemmel: src.tilgangshjemmel,
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
