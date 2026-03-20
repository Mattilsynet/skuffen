use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

#[async_trait]
pub trait RegistrerEksekveringUseCase: Send + Sync {
    async fn handle(&self, envelope: &CommandEnvelope<Command>) -> Result<(), anyhow::Error>;
}
