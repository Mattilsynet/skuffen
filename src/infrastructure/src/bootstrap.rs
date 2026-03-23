use application::command::ports::command_state_port::ArkivSakTilstandRepository;
use application::command::ports::eksekvering_port::ArkivGateway;
use application::query::services::hent_journalpost::HentJournalpostService;
use application::query::services::hent_sak::HentSakService;
use async_trait::async_trait;
use lib_nats::jetstream;
use tokio::task::JoinHandle;

use crate::command::adapter::eksekvering_state_postgres::PostgresEksekveringStateRepository;
use crate::command::adapter::fake_arkiv_gateway::FakeArkivGateway;
use crate::command::adapter::fake_command_state_repo::FakeArkivSakTilstandRepository;
use crate::command::adapter::id_mapping_postgres::PostgresIdMappingRepository;
use crate::command::adapter::nats_done_publisher::NatsDonePublisher;
use crate::command::adapter::nats_eksekvering_status_publisher::NatsEksekveringStatusPublisher;
use crate::command::adapter::nats_publisher::NatsCommandDispatcher;
use crate::command::adapter::nats_status_publisher::NatsCommandStatusPublisher;
use crate::command::adapter::nats_validated_publisher::NatsValidatedCommandDispatcher;
use crate::command::adapter::sikri_arkiv_gateway::SikriArkivGateway;
use crate::command::adapter::sikri_command_state_repo::SikriCommandStateRepository;
use crate::command::media::ObjectStoreMediaStore;
use crate::command::nats::command_listener::CommandListener;
use crate::command::nats::eksekvering_listener::KommandoEksekveringListener;
use crate::command::nats::validation_listener::CommandValidationListener;
use crate::http::health_check::health_check;
use crate::nats::client::NatsClient;
use crate::nats::setup::setup_nats;
use crate::query::adapter::fake_journalpost_repository::FakeJournalpostRepository;
use crate::query::adapter::fake_sak_repository::FakeSakRepository;
use crate::query::adapter::hent_sak::SikriRepository;
use crate::query::mapping::lookup::key_mapping_queries;
use crate::query::nats::listener::{NatsReplier, UseCase};
use crate::query::nats::query_listener::QueryListener;

pub struct RuntimeDeps {
    pub nats: NatsClient,
    pub health_check_handle: JoinHandle<()>,
    pub id_mapping_repo: PostgresIdMappingRepository,
    pub eksekvering_state_repo: PostgresEksekveringStateRepository,
    pub media_store: std::sync::Arc<ObjectStoreMediaStore>,
    pub use_fake_sikri: bool,
}

pub async fn prepare_runtime() -> anyhow::Result<RuntimeDeps> {
    let nats = setup_nats().await?;
    let health_check_handle = health_check().await?;
    let use_fake_sikri = use_fake_sikri();

    let db_pool = crate::database::setup::stup_database().await?;
    crate::database::setup::run_migrations(&db_pool).await?;

    let id_mapping_repo = PostgresIdMappingRepository::new(db_pool.clone());
    key_mapping_queries::init_id_mapping_repo(std::sync::Arc::new(id_mapping_repo.clone()));

    let eksekvering_state_repo = PostgresEksekveringStateRepository::new(db_pool);
    let media_store = setup_media_store(nats.clone()).await?;

    Ok(RuntimeDeps {
        nats,
        health_check_handle,
        id_mapping_repo,
        eksekvering_state_repo,
        media_store,
        use_fake_sikri,
    })
}

pub fn build_query_listener(nats: NatsClient, use_fake_sikri: bool) -> QueryListener {
    let hent_sak_uc = if use_fake_sikri {
        HentSakService::new(Box::new(FakeSakRepository::new()))
    } else {
        HentSakService::new(Box::new(SikriRepository))
    };
    //TODO: Ikke implementert endepunkt enda.
    let hent_journalpost_uc =
        HentJournalpostService::new(Box::new(FakeJournalpostRepository::new()));
    let hent_sak_replier = NatsReplier::new(nats.clone(), "sak.hent", Box::new(hent_sak_uc));
    let hent_journalpost_replier =
        NatsReplier::new(nats, "journalpost.hent", Box::new(hent_journalpost_uc));

    QueryListener::new(hent_sak_replier, hent_journalpost_replier)
}

pub fn build_ready_replier(nats: NatsClient) -> NatsReplier<String, String> {
    NatsReplier::<String, String>::new(nats, "skuffen.ready", Box::new(ReadyUseCase))
}

pub fn build_command_listener(
    nats: NatsClient,
    id_mapping_repo: PostgresIdMappingRepository,
    media_store: std::sync::Arc<ObjectStoreMediaStore>,
) -> CommandListener {
    let command_service = application::command::services::ingest_command::IngestCommandService::new(
        Box::new(id_mapping_repo),
        Box::new(NatsCommandDispatcher::new(nats.clone())),
    );

    CommandListener::new(nats, command_service, media_store)
}

pub fn build_validator_listener(
    nats: NatsClient,
    id_mapping_repo: PostgresIdMappingRepository,
    use_fake_sikri: bool,
) -> CommandValidationListener {
    let validator_service =
        application::command::services::validate_command::ValidateCommandService::new(
            command_state_repository(use_fake_sikri),
            Box::new(id_mapping_repo),
            Box::new(NatsValidatedCommandDispatcher::new(nats.clone())),
            Box::new(NatsCommandStatusPublisher::new(nats.clone())),
        );

    CommandValidationListener::new(nats, validator_service)
}

pub fn build_eksekvering_components(
    nats: NatsClient,
    id_mapping_repo: PostgresIdMappingRepository,
    eksekvering_state_repo: PostgresEksekveringStateRepository,
    media_store: std::sync::Arc<ObjectStoreMediaStore>,
    use_fake_sikri: bool,
) -> (
    KommandoEksekveringListener,
    application::command::services::eksekvering_worker::EksekveringWorker,
) {
    let registrer_eksekvering_service =
        application::command::services::registrer_eksekvering::RegistrerEksekveringService::new(
            Box::new(eksekvering_state_repo.clone()),
            Box::new(id_mapping_repo.clone()),
        );

    let eksekvering_service =
        application::command::services::eksekver_kommando::EksekverKommandoService::new(
            Box::new(eksekvering_state_repo.clone()),
            arkiv_gateway(use_fake_sikri, media_store),
            Box::new(NatsEksekveringStatusPublisher::new(nats.clone())),
            Box::new(NatsDonePublisher::new(nats.clone())),
            Box::new(id_mapping_repo.clone()),
        );

    let eksekvering_listener =
        KommandoEksekveringListener::new(nats, Box::new(registrer_eksekvering_service));
    let eksekvering_worker =
        application::command::services::eksekvering_worker::EksekveringWorker::new(
            Box::new(eksekvering_state_repo),
            eksekvering_service,
            "worker-1".to_string(),
            std::time::Duration::from_secs(5),
            10,
        );

    (eksekvering_listener, eksekvering_worker)
}

fn use_fake_sikri() -> bool {
    std::env::var("SKUFFEN_FAKE_SIKRI")
        .map(|value| value == "1")
        .unwrap_or(false)
}

async fn setup_media_store(
    nats: NatsClient,
) -> anyhow::Result<std::sync::Arc<ObjectStoreMediaStore>> {
    let jetstream = jetstream::new(nats.inner().clone());
    let store = match jetstream.get_object_store("arkiv_media").await {
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
    Ok(std::sync::Arc::new(ObjectStoreMediaStore::new(store)))
}

fn command_state_repository(use_fake_sikri: bool) -> Box<dyn ArkivSakTilstandRepository> {
    if use_fake_sikri {
        Box::new(FakeArkivSakTilstandRepository)
    } else {
        Box::new(SikriCommandStateRepository)
    }
}

fn arkiv_gateway(
    use_fake_sikri: bool,
    media_store: std::sync::Arc<ObjectStoreMediaStore>,
) -> Box<dyn ArkivGateway> {
    if use_fake_sikri {
        Box::new(FakeArkivGateway::new())
    } else {
        Box::new(SikriArkivGateway::new(media_store))
    }
}

struct ReadyUseCase;

#[async_trait]
impl UseCase<String, String> for ReadyUseCase {
    async fn handle(&self, _req: String) -> Result<String, anyhow::Error> {
        Ok("ready".to_string())
    }
}
