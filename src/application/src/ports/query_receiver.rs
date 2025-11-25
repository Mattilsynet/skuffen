use async_trait::async_trait;

#[async_trait]
pub trait QueryReceiver: Send + Sync + 'static {
    async fn get_all(&self) -> Result<Vec<Virksomhet>, anyhow::Error>;
}
