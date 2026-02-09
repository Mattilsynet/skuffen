use core::fmt;
use std::str::FromStr;

use anyhow::{anyhow, Result};
use uuid::Uuid;

use crate::model::{journalpost::Journalpost, tilgang::Tilgang};

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Sak {
    pub sakstittel: Sakstittel,
    pub saksbehandler: String,
    pub saksstatus: Saksstatus,
    pub tilgang: Option<Tilgang>,
    pub sak_key: SakKey,
    pub kildesystem: String,
    pub lukket: bool,
    pub journalposter: Vec<Journalpost>,
    pub ordningsverdi: Ordningsverdi,
}

/**
* SaksTittel benyttes på opprettelse av sak i arkiv
*/
const SAKSTITTEL_MAX_LENGTH: usize = 256;


#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Saksbehandler {
    pub saksbehandler_id: String,
    pub saksbehandler_enhet: String,
}

impl Saksbehandler {
    pub fn new(saksbehandler_id: String, saksbehandler_enhet: String) -> Result<Self> {
        if saksbehandler_id.is_empty() {
            return Err(anyhow!("Saksbehandler is empty"));
        }
        if saksbehandler_enhet.is_empty() {
            return Err(anyhow!("SaksbehandlerEnhet is empty"));
        }
        Ok(Self {
            saksbehandler_id,
            saksbehandler_enhet,
        })
    }
}



#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Sakstittel(pub String);

impl Sakstittel {
    pub fn uo_tittel(&self) -> Sakstittel {
        Sakstittel("*****".to_string())
    }
}

impl FromStr for Sakstittel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let trimmed = s.trim();

        if trimmed.is_empty() {
            return Err(anyhow!("Sakstittel er tom"));
        }

        if trimmed.len() > SAKSTITTEL_MAX_LENGTH {
            return Err(anyhow!(
                "Sakstittel er for lang. Max lengde: {SAKSTITTEL_MAX_LENGTH}"
            ));
        }

        Ok(Sakstittel(trimmed.to_string()))
    }
}

impl TryFrom<&str> for Sakstittel {
    type Error = anyhow::Error;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl TryFrom<String> for Sakstittel {
    type Error = anyhow::Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.as_str().parse()
    }
}

impl fmt::Display for Sakstittel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Ordningsverdi(String);

impl Ordningsverdi {
    pub fn new(s: String) -> Result<Self> {
        // non-empty
        if s.is_empty() {
            return Err(anyhow!("string is empty"));
        }

        // only digits or '-'
        if !s.chars().all(|c| c.is_ascii_digit() || c == '-') {
            return Err(anyhow!(format!("invalid character in '{s}'")));
        }

        // max 1 '-'
        let hyphen_count = s.chars().filter(|&c| c == '-').count();
        if hyphen_count > 1 {
            return Err(anyhow!("more than one '-' found".to_string()));
        }

        Ok(Ordningsverdi(s))
    }

    pub fn get(&self) -> &str {
        &self.0
    }
}
#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub struct SakKey {
    pub skuffen_id: Uuid,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum Saksstatus {
    UnderBehandling,
    Ferdig,
    Avsluttet,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum Arkivdel {
    // Tilsynsdivisjonene må bli mappet om til SAK og 
    // Hovedkontoret må bli mappet om til SAKHK 
    Tilsynsdivisjonene,
    Hovedkontoret,
}

#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub struct Saksnummer(pub String);

impl Saksnummer {
    /// Construct from a string of the form "YYYY/<seq>".
    /// - Year must be 4 digits and valid.
    /// - Sequence can be any non-empty string.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        let parts: Vec<&str> = s.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(anyhow!("Ugyldig format på saksnummer."));
        }

        let year_str = parts[0];
        let seq_str = parts[1];

        let year: u16 = year_str
            .parse()
            .map_err(|_| anyhow!("Ugyldig format på saksår."))?;
        if !(1000..=9999).contains(&year) {
            return Err(anyhow!("Ugyldig saksår."));
        }

        if seq_str.is_empty() {
            return Err(anyhow!("Ugyldig format på sekvensnummer."));
        }

        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn year(&self) -> u16 {
        self.0[0..4].parse().expect("validated year")
    }

    pub fn sequence(&self) -> &str {
        &self.0[5..]
    }
}
