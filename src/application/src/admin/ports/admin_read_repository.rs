use async_trait::async_trait;
use uuid::Uuid;

use crate::admin::model::{AdminCommand, AdminSak, AdminSakNokkel};

/// Read-only projection av lokal reparasjonstilstand.
///
/// Implementasjonen skal lese command og operasjoner, eller sak og barn, fra
/// samme konsistente snapshot. Den skriver aldri.
#[async_trait]
pub trait AdminReadRepository: Send + Sync {
    async fn hent_command(&self, command_id: Uuid) -> Result<Option<AdminCommand>, anyhow::Error>;

    async fn hent_sak(&self, key: AdminSakNokkel) -> Result<Option<AdminSak>, anyhow::Error>;
}
