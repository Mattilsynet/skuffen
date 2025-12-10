use anyhow::{anyhow, Result};
use uuid::Uuid;

use crate::model::journalpost::Journalpost;

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Sak {
    pub sakstittel: String,
    pub saksbehandler: String,
    pub saksstatus: Saksstatus,
    pub unntatt_offentlighet: bool,
    pub saksnr: Saksnummer,
    pub kildesystem: String,
    pub lukket: bool,
    pub journalposter: Vec<Journalpost>,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum SakKey {
    SkuffenId(Uuid),
    ArkivId(Saksnummer),
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum Saksstatus {
    UnderBehandling,
    Ferdig,
    Avsluttet,
}

#[derive(PartialEq, Eq, Debug, Clone)]
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
