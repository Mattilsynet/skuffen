use lib_schemas::skuffen::query::queries::{HentJournalpostQuery, HentSakQuery};
use lib_schemas::skuffen::query::responses::{JournalpostResponse, SakResponse};

use crate::query::nats::listener::{BrukerMtEnheterRequest, BrukerMtEnheterResponse, NatsReplier};

pub struct QueryListener {
    hent_sak_replier: NatsReplier<HentSakQuery, SakResponse>,
    hent_journalpost_replier: NatsReplier<HentJournalpostQuery, JournalpostResponse>,
    bruker_mt_enheter_replier: NatsReplier<BrukerMtEnheterRequest, BrukerMtEnheterResponse>,
}

impl QueryListener {
    pub fn new(
        hent_sak_replier: NatsReplier<HentSakQuery, SakResponse>,
        hent_journalpost_replier: NatsReplier<HentJournalpostQuery, JournalpostResponse>,
        bruker_mt_enheter_replier: NatsReplier<BrukerMtEnheterRequest, BrukerMtEnheterResponse>,
    ) -> Self {
        Self {
            hent_sak_replier,
            hent_journalpost_replier,
            bruker_mt_enheter_replier,
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let (sak_result, journalpost_result, bruker_mt_enheter_result) = tokio::join!(
            self.hent_sak_replier.run(),
            self.hent_journalpost_replier.run(),
            self.bruker_mt_enheter_replier.run()
        );
        sak_result?;
        journalpost_result?;
        bruker_mt_enheter_result?;
        Ok(())
    }
}
