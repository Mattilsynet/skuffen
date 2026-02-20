use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStateErrorKind {
    Recoverable,
    Irrecoverable,
}

#[derive(Debug)]
pub struct CommandStateError {
    pub kind: CommandStateErrorKind,
    pub message: String,
}

impl CommandStateError {
    pub fn new(kind: CommandStateErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CommandStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CommandStateError {}

#[derive(Debug, Clone, Copy)]
pub struct SakState {
    pub avsluttet: bool,
}

#[async_trait]
pub trait CommandStateRepository: Send + Sync {
    async fn hent_sak_state(&self, saksnummer: &str) -> Result<SakState, CommandStateError>;
}
