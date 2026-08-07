//! Nasjonale identifikatorer for korrespondanseparter.
//!
//! Validerte verdi-objekter. Reglene speiler den eksterne wire-kontrakten
//! (`lib-schemas`), men domenet eier sin egen validering og er uavhengig av
//! kontrakt-typene. Etter konstruksjon kan verdiene stoles på.

use anyhow::{Result, anyhow};

/// Norsk fødselsnummer (11 siffer med gyldige kontrollsifre K1/K2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fødselsnummer(String);

impl Fødselsnummer {
    pub fn new(fnr: impl Into<String>) -> Result<Self> {
        let fnr = fnr.into();
        if !gyldig_fødselsnummer(&fnr) {
            return Err(anyhow!("ugyldig fødselsnummer"));
        }
        Ok(Self(fnr))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Norsk organisasjonsnummer (9 siffer med gyldig kontrollsiffer, modulus 11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Organisasjonsnummer(String);

impl Organisasjonsnummer {
    pub fn new(orgnr: impl Into<String>) -> Result<Self> {
        let orgnr = orgnr.into();
        if !gyldig_organisasjonsnummer(&orgnr) {
            return Err(anyhow!("ugyldig organisasjonsnummer"));
        }
        Ok(Self(orgnr))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Nasjonal identifikator: enten et fødselsnummer eller et organisasjonsnummer.
///
/// Variantene har ulik struktur og bærer derfor hver sin validerte newtype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NasjonalId {
    Fødselsnummer(Fødselsnummer),
    Organisasjonsnummer(Organisasjonsnummer),
}

/// Norsk postnummer (4 siffer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Postnummer(String);

impl Postnummer {
    pub fn new(postnummer: impl Into<String>) -> Result<Self> {
        let postnummer = postnummer.into();
        if postnummer.len() != 4 || !postnummer.chars().all(|c| c.is_ascii_digit()) {
            return Err(anyhow!(
                "ugyldig postnummer '{postnummer}'; forventet 4 siffer"
            ));
        }
        Ok(Self(postnummer))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn gyldig_fødselsnummer(fnr: &str) -> bool {
    if fnr.len() != 11 || !fnr.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    let d: Vec<u32> = fnr.chars().filter_map(|c| c.to_digit(10)).collect();

    let weights1 = [3, 7, 6, 1, 8, 9, 4, 5, 2, 1];
    let sum1: u32 = weights1.iter().zip(&d).map(|(w, d)| w * d).sum();
    if !sum1.is_multiple_of(11) {
        return false;
    }

    let weights2 = [5, 4, 3, 2, 7, 6, 5, 4, 3, 2, 1];
    let sum2: u32 = weights2.iter().zip(&d).map(|(w, d)| w * d).sum();
    sum2.is_multiple_of(11)
}

fn gyldig_organisasjonsnummer(orgnr: &str) -> bool {
    if orgnr.len() != 9 || !orgnr.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    let d: Vec<u32> = orgnr.chars().filter_map(|c| c.to_digit(10)).collect();
    let weights = [3, 2, 7, 6, 5, 4, 3, 2];
    let sum: u32 = weights.iter().zip(&d).map(|(w, d)| w * d).sum();
    let rem = sum % 11;
    let k = if rem == 0 { 0 } else { 11 - rem };

    k != 10 && k == d[8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gyldig_fnr_aksepteres() {
        assert!(Fødselsnummer::new("01010101006").is_ok());
    }

    #[test]
    fn ugyldig_fnr_avvises() {
        assert!(Fødselsnummer::new("01010101007").is_err());
        assert!(Fødselsnummer::new("123").is_err());
        assert!(Fødselsnummer::new("abcdefghijk").is_err());
    }

    #[test]
    fn gyldig_orgnr_aksepteres() {
        assert!(Organisasjonsnummer::new("995298775").is_ok());
    }

    #[test]
    fn ugyldig_orgnr_avvises() {
        assert!(Organisasjonsnummer::new("995298776").is_err());
        assert!(Organisasjonsnummer::new("123").is_err());
    }

    #[test]
    fn postnummer_krever_fire_siffer() {
        assert!(Postnummer::new("0350").is_ok());
        assert!(Postnummer::new("035").is_err());
        assert!(Postnummer::new("03500").is_err());
        assert!(Postnummer::new("ABCD").is_err());
    }
}
