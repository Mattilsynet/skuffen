use async_trait::async_trait;

use crate::command::{Command, CommandEnvelope};

#[async_trait]
pub trait ValidatedCommandDispatcher: Send + Sync {
    async fn dispatch_validated(
        &self,
        command: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error>;
}
