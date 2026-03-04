fn init_crypto() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("install aws-lc-rs provider");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_crypto();
    dotenvy::dotenv().ok();
    infrastructure::telemetry::init_observability();

    let runtime = infrastructure::bootstrap::prepare_runtime().await?;

    let hent_sak_replier = infrastructure::bootstrap::build_hent_sak_replier(
        runtime.nats.clone(),
        runtime.use_fake_sikri,
    );
    let hent_journalpost_replier =
        infrastructure::bootstrap::build_hent_journalpost_replier(runtime.nats.clone());
    let ready_replier = infrastructure::bootstrap::build_ready_replier(runtime.nats.clone());

    let media_listener = infrastructure::command::nats::media_listener::MediaListener::new(
        runtime.nats.clone(),
        runtime.media_store.clone(),
    );
    let command_listener = infrastructure::bootstrap::build_command_listener(
        runtime.nats.clone(),
        runtime.id_mapping_repo.clone(),
        runtime.media_store.clone(),
    );
    let validator_listener = infrastructure::bootstrap::build_validator_listener(
        runtime.nats.clone(),
        runtime.id_mapping_repo.clone(),
        runtime.use_fake_sikri,
    );
    let (eksekvering_listener, eksekvering_worker) =
        infrastructure::bootstrap::build_eksekvering_components(
            runtime.nats.clone(),
            runtime.id_mapping_repo,
            runtime.eksekvering_state_repo,
            runtime.media_store,
            runtime.use_fake_sikri,
        );

    let _ = tokio::join!(
        runtime.health_check_handle,
        hent_sak_replier.run(),
        hent_journalpost_replier.run(),
        ready_replier.run(),
        media_listener.run(),
        command_listener.run(),
        validator_listener.run(),
        eksekvering_listener.run(),
        eksekvering_worker.run(),
    );

    Ok(())
}
