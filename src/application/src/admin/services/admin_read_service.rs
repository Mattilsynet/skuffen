use std::sync::Arc;

use uuid::Uuid;

use crate::admin::model::{AdminCommand, AdminSak, AdminSakNokkel};
use crate::admin::ports::admin_read_repository::AdminReadRepository;

/// Typet utfall, slik at transportlaget kan skille not-found fra intern feil
/// uten å sammenligne feilstrenger.
#[derive(Debug, thiserror::Error)]
pub enum AdminReadError {
    #[error("command not found")]
    CommandNotFound,
    #[error("sak not found")]
    SakNotFound,
    #[error("admin read repository failed")]
    Repository(#[from] anyhow::Error),
}

/// `utfort_av` er transportattribusjon og har ingen plass her.
pub struct AdminReadService {
    repository: Arc<dyn AdminReadRepository>,
}

impl AdminReadService {
    pub fn new(repository: Arc<dyn AdminReadRepository>) -> Self {
        Self { repository }
    }

    pub async fn hent_command(&self, command_id: Uuid) -> Result<AdminCommand, AdminReadError> {
        self.repository
            .hent_command(command_id)
            .await?
            .ok_or(AdminReadError::CommandNotFound)
    }

    pub async fn hent_sak(&self, key: AdminSakNokkel) -> Result<AdminSak, AdminReadError> {
        self.repository
            .hent_sak(key)
            .await?
            .ok_or(AdminReadError::SakNotFound)
    }
}
