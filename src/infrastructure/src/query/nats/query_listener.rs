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

    /// `try_join!`, aldri `join!` (SKU-0021 R7). `join!` venter på alle tre,
    /// så én avsluttet subscription ville blokkert for alltid uten at
    /// supervisoren fikk vite det.
    pub async fn run(&self) -> anyhow::Result<()> {
        tokio::try_join!(
            self.hent_sak_replier.run(),
            self.hent_journalpost_replier.run(),
            self.bruker_mt_enheter_replier.run()
        )?;
        Ok(())
    }
}
