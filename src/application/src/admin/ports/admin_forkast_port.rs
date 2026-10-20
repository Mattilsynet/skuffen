use async_trait::async_trait;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ForkastFeil {
    #[error("command not found")]
    CommandNotFound(Vec<Uuid>),
    #[error("admin forkast repository failed")]
    Repository(#[from] anyhow::Error),
}

#[async_trait]
pub trait AdminForkastRepository: Send + Sync {
    /// Markerer kommandoene som forkastet, unntatt fullførte. Ukjent
    /// command-id avviser hele forespørselen uten endringer.
    async fn forkast(&self, command_ids: &[Uuid]) -> Result<(), ForkastFeil>;
}
