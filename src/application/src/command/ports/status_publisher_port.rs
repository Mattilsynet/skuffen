use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::CommandStatusEvent;

#[async_trait]
pub trait CommandStatusPublisher: Send + Sync {
    async fn publish_status(&self, event: CommandStatusEvent) -> Result<(), anyhow::Error>;
}
