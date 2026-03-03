use application::query::services::hent_journalpost::HentJournalpostService;
use application::query::services::hent_sak::HentSakService;
use async_trait::async_trait;
use infrastructure::command::adapter::id_mapping_postgres::PostgresIdMappingRepository;
use infrastructure::command::nats::media_listener::MediaListener;
use infrastructure::query::mapping::lookup::key_mapping_queries;
use infrastructure::query::nats::listener::UseCase;
use infrastructure::{
    command::adapter::{
        eksekvering_state_postgres::PostgresEksekveringStateRepository,
        fake_arkiv_gateway::FakeArkivGateway, fake_command_state_repo::FakeCommandStateRepository,
        nats_done_publisher::NatsDonePublisher,
        nats_eksekvering_status_publisher::NatsEksekveringStatusPublisher,
        nats_publisher::NatsCommandDispatcher, nats_status_publisher::NatsCommandStatusPublisher,
        nats_validated_publisher::NatsValidatedCommandDispatcher,
        sikri_arkiv_gateway::SikriArkivGateway,
        sikri_command_state_repo::SikriCommandStateRepository,
    },
    command::media::ObjectStoreMediaStore,
    http::health_check::health_check,
    nats::setup::setup_nats,
    query::adapter::fake_journalpost_repository::FakeJournalpostRepository,
    query::adapter::fake_sak_repository::FakeSakRepository,
    query::adapter::hent_sak::SikriRepository,
    query::nats::listener::NatsReplier,
    telemetry::{get_subscriber, init_subscriber},
};
use lib_nats::jetstream;
use lib_schemas::skuffen::query::queries::HentJournalpostQuery;
use lib_schemas::skuffen::query::queries::HentSakQuery;
use lib_schemas::skuffen::query::responses::{JournalpostResponse, SakResponse};

fn init_crypto() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("install aws-lc-rs provider");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_crypto();
    dotenvy::dotenv().ok();
    let subscriber = get_subscriber();
    init_subscriber(subscriber);
    let nats = setup_nats().await?;

    let health_check_handle = health_check().await?;
    // let sak_repo = SakRepository::new(SikriRepository);
    // let jp_repo = JournalpostRepository::new(SikriRepository);

    let use_fake_sikri = std::env::var("SKUFFEN_FAKE_SIKRI")
        .map(|value| value == "1")
        .unwrap_or(false);
    let hent_sak_uc = if use_fake_sikri {
        HentSakService::new(Box::new(FakeSakRepository::new()))
    } else {
        HentSakService::new(Box::new(SikriRepository))
    };
    let hent_jp_uc = HentJournalpostService::new(Box::new(FakeJournalpostRepository::new()));
    let hent_sak_replier = NatsReplier::<HentSakQuery, SakResponse>::new(
        nats.clone(),
        "sak.hent",
        Box::new(hent_sak_uc),
    );
    let hent_journalpost_replier = NatsReplier::<HentJournalpostQuery, JournalpostResponse>::new(
        nats.clone(),
        "journalpost.hent",
        Box::new(hent_jp_uc),
    );

    // Command Ingestion Wiring
    let db_pool = infrastructure::database::setup::stup_database().await?;
    infrastructure::database::setup::run_migrations(&db_pool).await?;
    let id_mapping_repo = PostgresIdMappingRepository::new(db_pool.clone());

    key_mapping_queries::init_id_mapping_repo(std::sync::Arc::new(id_mapping_repo.clone()));

    let nats_dispatcher = NatsCommandDispatcher::new(nats.clone());

    let command_service = application::command::services::ingest_command::IngestCommandService::new(
        Box::new(id_mapping_repo.clone()),
        Box::new(nats_dispatcher),
    );

    let jetstream = jetstream::new(nats.clone().inner().clone());
    let media_store = match jetstream.get_object_store("arkiv_media").await {
        Ok(store) => store,
        Err(_) => {
            jetstream
                .create_object_store(jetstream::object_store::Config {
                    bucket: "arkiv_media".to_string(),
                    ..Default::default()
                })
                .await?
        }
    };
    let media_store = std::sync::Arc::new(ObjectStoreMediaStore::new(media_store));
    let media_listener = MediaListener::new(nats.clone(), media_store.clone());
    let command_listener = infrastructure::command::nats::command_listener::CommandListener::new(
        nats.clone(),
        command_service,
        media_store,
    );

    let command_state_repo: Box<
        dyn application::command::ports::command_state_port::CommandStateRepository,
    > = if use_fake_sikri {
        Box::new(FakeCommandStateRepository)
    } else {
        Box::new(SikriCommandStateRepository)
    };
    let validator_service =
        application::command::services::validate_command::ValidateCommandService::new(
            command_state_repo,
            Box::new(id_mapping_repo.clone()),
            Box::new(NatsValidatedCommandDispatcher::new(nats.clone())),
            Box::new(NatsCommandStatusPublisher::new(nats.clone())),
        );

    let validator_listener =
        infrastructure::command::nats::validation_listener::CommandValidationListener::new(
            nats.clone(),
            validator_service,
        );

    let eksekvering_state_repo = PostgresEksekveringStateRepository::new(db_pool.clone());
    let arkiv_gateway: Box<dyn application::command::ports::eksekvering_port::ArkivGateway> =
        if use_fake_sikri {
            Box::new(FakeArkivGateway::new())
        } else {
            Box::new(SikriArkivGateway::new())
        };
    let eksekvering_service =
        application::command::services::eksekver_kommando::EksekverKommandoService::new(
            Box::new(eksekvering_state_repo.clone()),
            arkiv_gateway,
            Box::new(NatsEksekveringStatusPublisher::new(nats.clone())),
            Box::new(NatsDonePublisher::new(nats.clone())),
            Box::new(id_mapping_repo.clone()),
        );
    let eksekvering_listener =
        infrastructure::command::nats::eksekvering_listener::KommandoEksekveringListener::new(
            nats.clone(),
            Box::new(eksekvering_state_repo.clone()),
            id_mapping_repo.clone(),
        );

    let eksekvering_worker =
        application::command::services::eksekvering_worker::EksekveringWorker::new(
            Box::new(eksekvering_state_repo),
            eksekvering_service,
            "worker-1".to_string(),
            std::time::Duration::from_secs(5),
            10,
        );

    // let receiver_handle =
    // let processor_handle =
    let ready_replier =
        NatsReplier::<String, String>::new(nats.clone(), "skuffen.ready", Box::new(ReadyUseCase));

    let _ = tokio::join!(
        health_check_handle,
        hent_sak_replier.run(),
        hent_journalpost_replier.run(),
        ready_replier.run(),
        media_listener.run(),
        command_listener.run(),
        validator_listener.run(),
        eksekvering_listener.run(),
        eksekvering_worker.run(),
        // receiver_handle,
        // processor_handle,
    );
    Ok(())
}

struct ReadyUseCase;

#[async_trait]
impl UseCase<String, String> for ReadyUseCase {
    async fn handle(&self, _req: String) -> Result<String, anyhow::Error> {
        Ok("ready".to_string())
    }
}
