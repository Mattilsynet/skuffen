use async_trait::async_trait;
use domain::eksekvering::typer::CommandLifecycleEvent;

#[async_trait]
pub trait CommandStatusPublisher: Send + Sync {
    async fn publish_status(&self, event: CommandLifecycleEvent) -> Result<(), anyhow::Error>;
}
