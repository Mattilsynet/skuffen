use application::query::services::hent_sak::HentSakService;
use infrastructure::{
    command::adapter::{
        nats_publisher::NatsCommandDispatcher,
        nats_status_publisher::NatsCommandStatusPublisher,
        nats_validated_publisher::NatsValidatedCommandDispatcher,
        sikri_command_state_repo::SikriCommandStateRepository,
    },
    http::health_check::health_check,
    nats::setup::setup_nats,
    query::adapter::hent_sak::SikriRepository,
    query::nats::listener::NatsReplier,
    telemetry::{get_subscriber, init_subscriber},
};
use infrastructure::command::adapter::id_mapping_postgres::PostgresIdMappingRepository;
use infrastructure::query::mapping::lookup::key_mapping_queries;
use lib_schemas::skuffen::query::queries::HentSakQuery;
use lib_schemas::skuffen::query::responses::SakResponse;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let subscriber = get_subscriber();
    init_subscriber(subscriber);
    let nats = setup_nats().await?;

    let health_check_handle = health_check().await?;
    // let sak_repo = SakRepository::new(SikriRepository);
    // let jp_repo = JournalpostRepository::new(SikriRepository);

    let hent_sak_uc = HentSakService::new(Box::new(SikriRepository));
    // let hent_jp_uc = HentJournalpostService::new(jp_repo);
    let hent_sak_replier = NatsReplier::<HentSakQuery, SakResponse>::new(
        nats.clone(),
        "sak.hent",
        Box::new(hent_sak_uc),
    );

    // Command Ingestion Wiring
    let db_pool = infrastructure::database::setup::stup_database().await?;
    let id_mapping_repo = PostgresIdMappingRepository::new(db_pool);

    key_mapping_queries::init_id_mapping_repo(std::sync::Arc::new(id_mapping_repo.clone()));

    let nats_dispatcher = NatsCommandDispatcher::new(nats.clone());

    let command_service = application::command::services::ingest_command::IngestCommandService::new(
        Box::new(id_mapping_repo),
        Box::new(nats_dispatcher),
    );

    let command_listener = infrastructure::command::nats::command_listener::CommandListener::new(
        nats.clone(),
        command_service,
    );

    let validator_service = application::command::services::validate_command::ValidateCommandService::new(
        Box::new(SikriCommandStateRepository),
        Box::new(id_mapping_repo.clone()),
        Box::new(NatsValidatedCommandDispatcher::new(nats.clone())),
        Box::new(NatsCommandStatusPublisher::new(nats.clone())),
    );

    let validator_listener =
        infrastructure::command::nats::validation_listener::CommandValidationListener::new(
            nats.clone(),
            validator_service,
        );

    // let receiver_handle =
    // let processor_handle =
    let _ = tokio::join!(
        health_check_handle,
        hent_sak_replier.run(),
        command_listener.run(),
        validator_listener.run(),
        // receiver_handle,
        // processor_handle,
    );
    Ok(())
}
