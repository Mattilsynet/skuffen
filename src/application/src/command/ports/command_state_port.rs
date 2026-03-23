use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArkivSakTilstandErrorKind {
    Recoverable,
    Irrecoverable,
}

#[derive(Debug)]
pub struct ArkivSakTilstandError {
    pub kind: ArkivSakTilstandErrorKind,
    pub message: String,
}

impl ArkivSakTilstandError {
    pub fn new(kind: ArkivSakTilstandErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ArkivSakTilstandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ArkivSakTilstandError {}

#[derive(Debug, Clone, Copy)]
pub struct ArkivSakTilstand {
    pub avsluttet: bool,
}

#[async_trait]
pub trait ArkivSakTilstandRepository: Send + Sync {
    async fn hent_sak_tilstand_fra_arkivet(
        &self,
        saksnummer: &str,
    ) -> Result<ArkivSakTilstand, ArkivSakTilstandError>;
}
