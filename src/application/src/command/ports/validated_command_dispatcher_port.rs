use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

#[async_trait]
pub trait ValidatedCommandDispatcher: Send + Sync {
    async fn dispatch_validated(
        &self,
        command: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error>;
}
