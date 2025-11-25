use std::net::SocketAddr;

use application::services::hent_sak::HentSakService;
use infrastructure::{
    adapter::hent_sak::SikriRepository,
    http::health_check::health_check,
    nats::{listener::NatsReplier, setup::setup_nats},
    telemetry::{get_subscriber, init_subscriber},
};
use lib_schemas::arkiv::v2::sak::{HentSakRequest, SakResponse};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let subscriber = get_subscriber();
    init_subscriber(subscriber);
    let nats = setup_nats().await?;

    let addr: SocketAddr = "0.0.0.0:8080".parse().unwrap();
    let health_check_handle = health_check(addr).await?;
    // let sak_repo = SakRepository::new(SikriRepository);
    // let jp_repo = JournalpostRepository::new(SikriRepository);

    let hent_sak_uc = HentSakService::new(SikriRepository);
    // let hent_jp_uc = HentJournalpostService::new(jp_repo);
    let hent_sak_replier =
        NatsReplier::<_, HentSakRequest, SakResponse>::new(nats.clone(), "sak.hent", hent_sak_uc);

    // let hent_jp_replier = NatsReplier::<_, HentJournalpostRequest, JournalpostResponse>::new(
    //     nats.clone(),
    //     "journalpost.hent",
    //     hent_jp_uc,
    // );

    // let receiver_handle =
    // let processor_handle =
    let _ = tokio::join!(
        health_check_handle,
        hent_sak_replier.run(),
        // hent_jp_replier,
        // receiver_handle,
        // processor_handle,
    );
    Ok(())
}
