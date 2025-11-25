use async_trait::async_trait;

#[async_trait]
pub trait JournalpostPort {
    async fn hent(
        &self,
        journalpost_id: domain::model::journalpost::JournalpostId,
    ) -> Result<domain::model::journalpost::Journalpost, anyhow::Error>;
    async fn opprett(); //TODO
    async fn journalfoer(); //TODO
    async fn avskriv(); //TODO
}
