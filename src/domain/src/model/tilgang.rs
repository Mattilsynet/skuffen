use anyhow::{Result, anyhow};

/// Tilgangskode som uttrykker skjerming. Validert, ikke-tom verdi.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Tilgangskode(String);

impl Tilgangskode {
    pub fn new(kode: impl Into<String>) -> Result<Self> {
        let kode = kode.into();
        if kode.trim().is_empty() {
            return Err(anyhow!("tilgangskode er tom"));
        }
        Ok(Self(kode))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Rettslig hjemmel for skjerming. Validert, ikke-tom verdi.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Tilgangshjemmel(String);

impl Tilgangshjemmel {
    pub fn new(hjemmel: impl Into<String>) -> Result<Self> {
        let hjemmel = hjemmel.into();
        if hjemmel.trim().is_empty() {
            return Err(anyhow!("tilgangshjemmel er tom"));
        }
        Ok(Self(hjemmel))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Skjermingsmetadata for saker og journalposter. Kode og hjemmel hører sammen
/// og er begge validerte, ikke-tomme verdier.
#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Tilgang {
    pub tilgangskode: Tilgangskode,
    pub tilgangshjemmel: Tilgangshjemmel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tom_tilgangskode_avvises() {
        assert!(Tilgangskode::new("").is_err());
        assert!(Tilgangskode::new("   ").is_err());
        assert!(Tilgangskode::new("UO").is_ok());
    }

    #[test]
    fn tom_tilgangshjemmel_avvises() {
        assert!(Tilgangshjemmel::new("").is_err());
        assert!(Tilgangshjemmel::new("Offl. § 13").is_ok());
    }
}
