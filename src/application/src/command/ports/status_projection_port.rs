use async_trait::async_trait;
use domain::eksekvering::typer::CommandLifecycleContext;

use crate::command::{Command, CommandEnvelope};
#[async_trait]
pub trait CommandOutwardStatusProjector: Send + Sync {
    async fn resolve_context(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<CommandLifecycleContext, anyhow::Error>;
}
