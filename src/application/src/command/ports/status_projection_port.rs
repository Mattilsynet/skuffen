use async_trait::async_trait;
use domain::eksekvering::typer::CommandLifecycleContext;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

#[async_trait]
pub trait CommandOutwardStatusProjector: Send + Sync {
    async fn resolve_context(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<CommandLifecycleContext, anyhow::Error>;
}
