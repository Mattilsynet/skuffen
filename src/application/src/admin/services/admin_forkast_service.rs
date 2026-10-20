use std::collections::HashSet;
use std::sync::Arc;

use uuid::Uuid;

use crate::admin::ports::admin_forkast_port::{AdminForkastRepository, ForkastFeil};
use crate::admin::services::admin_retry_service::MAKS_COMMANDS_PER_RETRY;

#[derive(Debug, thiserror::Error)]
pub enum AdminForkastError {
    #[error("ugyldig antall kommandoer")]
    UgyldigAntall,
    #[error("command not found")]
    CommandNotFound(Vec<Uuid>),
    #[error("admin forkast failed")]
    Teknisk(#[from] anyhow::Error),
}

/// Operatøren forkaster kommandoer Skuffen ikke skal gjøre mer med
/// (SKU-0024). Ingenting reverseres; operatøren tar ansvaret.
pub struct AdminForkastService {
    repository: Arc<dyn AdminForkastRepository>,
}

impl AdminForkastService {
    pub fn new(repository: Arc<dyn AdminForkastRepository>) -> Self {
        Self { repository }
    }

    #[tracing::instrument(
        skip_all,
        name = "admin.forkast",
        fields(antall_commands = command_ids.len())
    )]
    pub async fn handle(&self, command_ids: Vec<Uuid>) -> Result<(), AdminForkastError> {
        let mut sett = HashSet::new();
        let command_ids: Vec<Uuid> = command_ids
            .into_iter()
            .filter(|id| sett.insert(*id))
            .collect();
        if command_ids.is_empty() || command_ids.len() > MAKS_COMMANDS_PER_RETRY {
            return Err(AdminForkastError::UgyldigAntall);
        }

        match self.repository.forkast(&command_ids).await {
            Ok(()) => Ok(()),
            Err(ForkastFeil::CommandNotFound(ids)) => Err(AdminForkastError::CommandNotFound(ids)),
            Err(ForkastFeil::Repository(err)) => Err(AdminForkastError::Teknisk(err)),
        }
    }
}
