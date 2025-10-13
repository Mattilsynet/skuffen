use async_trait::async_trait;

#[async_trait]
pub trait VedleggPort {
    async fn legg_til();
}
