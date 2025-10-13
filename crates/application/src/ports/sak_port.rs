use async_trait::async_trait;

#[async_trait]
pub trait SakPort {
    async fn hent();
    async fn opprett();
    async fn avslutt();
}
