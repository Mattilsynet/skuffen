use async_trait::async_trait;
use domain::eksekvering::typer::StatusErrorCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArkivSakTilstandErrorKind {
    Recoverable,
    Irrecoverable,
}

/// `message` og `error_code` går videre til klienten uendret; `kode` er den
/// stabile, greppbare varianten for logg.
#[derive(Debug)]
pub struct ArkivSakTilstandError {
    pub kind: ArkivSakTilstandErrorKind,
    pub kode: &'static str,
    pub message: String,
    pub error_code: StatusErrorCode,
}

impl ArkivSakTilstandError {
    pub fn new(
        kind: ArkivSakTilstandErrorKind,
        kode: &'static str,
        message: impl Into<String>,
        error_code: StatusErrorCode,
    ) -> Self {
        Self {
            kind,
            kode,
            message: message.into(),
            error_code,
        }
    }

    /// Arkivet er ikke nåbart. Alltid recoverable.
    pub fn utilgjengelig() -> Self {
        Self::new(
            ArkivSakTilstandErrorKind::Recoverable,
            "sikri_upstream_unavailable",
            "Sikri/Elements er midlertidig utilgjengelig. Prøv igjen senere.",
            StatusErrorCode::TemporaryUnavailable,
        )
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
