use async_trait::async_trait;

use crate::command::{Command, CommandEnvelope};

#[async_trait]
pub trait CommandDispatcher: Send + Sync {
    async fn dispatch(&self, command: &CommandEnvelope<Command>) -> Result<(), anyhow::Error>;
}
