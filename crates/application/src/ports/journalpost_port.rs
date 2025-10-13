use async_trait::async_trait;

#[async_trait]
pub trait JournalpostPort {
    async fn hent();
    async fn opprett();
    async fn journalfoer();
    async fn avskriv();
}
