use lib_schemas::skuffen::query::queries::{HentJournalpostQuery, HentSakQuery};
use lib_schemas::skuffen::query::responses::{JournalpostResponse, SakResponse};

use crate::query::nats::listener::NatsReplier;

pub struct QueryListener {
    hent_sak_replier: NatsReplier<HentSakQuery, SakResponse>,
    hent_journalpost_replier: NatsReplier<HentJournalpostQuery, JournalpostResponse>,
}

impl QueryListener {
    pub fn new(
        hent_sak_replier: NatsReplier<HentSakQuery, SakResponse>,
        hent_journalpost_replier: NatsReplier<HentJournalpostQuery, JournalpostResponse>,
    ) -> Self {
        Self {
            hent_sak_replier,
            hent_journalpost_replier,
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let (sak_result, journalpost_result) = tokio::join!(
            self.hent_sak_replier.run(),
            self.hent_journalpost_replier.run()
        );
        sak_result?;
        journalpost_result?;
        Ok(())
    }
}
