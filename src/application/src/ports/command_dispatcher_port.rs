use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{CommandEnvelope, Kommando};

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait CommandDispatcher: Send + Sync {
    async fn dispatch(&self, command: &CommandEnvelope<Kommando>) -> Result<(), anyhow::Error>;
}
