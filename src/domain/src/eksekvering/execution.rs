#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EksekveringStatus {
    Klar,
    Kjorer,
    BlokkertVenter,
    RetryVenter,
    Ok,
    Feil,
}

impl EksekveringStatus {
    pub fn as_db_code(self) -> &'static str {
        match self {
            Self::Klar => "klar",
            Self::Kjorer => "kjorer",
            Self::BlokkertVenter => "blokkert_venter",
            Self::RetryVenter => "retry_venter",
            Self::Ok => "ok",
            Self::Feil => "feil",
        }
    }
}
