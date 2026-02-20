use std::time::Duration;

use anyhow::Result;
use async_nats::jetstream;
use bytes::Bytes;
use futures::StreamExt;
use lib_nats::chunked_upload::protocol::{
    build_chunk_headers, split_payload, ChunkedUploadConfig, UploadMetadata,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::dokument::Dokument;
use lib_schemas::skuffen::query::queries::SakKey;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use testcontainers::{
    core::WaitFor, runners::AsyncRunner, ContainerAsync, GenericImage, RunnableImage,
};
use testcontainers_modules::postgres::Postgres;
use tokio::task::JoinHandle;
use uuid::Uuid;

use application::command::ports::command_state_port::{
    CommandStateError, CommandStateRepository, SakState,
};
use application::command::ports::eksekvering_port::{
    ArkivGateway, OpprettJournalpostResultat, Utsendingsvalg,
};
use application::command::services::eksekver_kommando::EksekverKommandoService;
use application::command::services::eksekvering_worker::EksekveringWorker;
use application::command::services::ingest_command::IngestCommandService;
use application::command::services::validate_command::ValidateCommandService;
use infrastructure::command::adapter::eksekvering_state_postgres::PostgresEksekveringStateRepository;
use infrastructure::command::adapter::id_mapping_postgres::PostgresIdMappingRepository;
use infrastructure::command::adapter::nats_done_publisher::NatsDonePublisher;
use infrastructure::command::adapter::nats_eksekvering_status_publisher::NatsEksekveringStatusPublisher;
use infrastructure::command::adapter::nats_publisher::NatsCommandDispatcher;
use infrastructure::command::adapter::nats_status_publisher::NatsCommandStatusPublisher;
use infrastructure::command::adapter::nats_validated_publisher::NatsValidatedCommandDispatcher;
use infrastructure::command::adapter::sikri_arkiv_gateway::SikriArkivGateway;
use infrastructure::command::adapter::sikri_command_state_repo::SikriCommandStateRepository;
use infrastructure::command::media::ObjectStoreMediaStore;
use infrastructure::command::nats::command_listener::CommandListener;
use infrastructure::command::nats::eksekvering_listener::KommandoEksekveringListener;
use infrastructure::command::nats::media_listener::MediaListener;
use infrastructure::command::nats::validation_listener::CommandValidationListener;
use infrastructure::nats::client::NatsClient;

struct FakeCommandStateRepository;

#[async_trait::async_trait]
impl CommandStateRepository for FakeCommandStateRepository {
    async fn hent_sak_state(&self, _saksnummer: &str) -> Result<SakState, CommandStateError> {
        Ok(SakState { avsluttet: false })
    }
}

#[derive(Default, Clone)]
struct FakeArkivGateway;

#[async_trait::async_trait]
impl ArkivGateway for FakeArkivGateway {
    async fn opprett_sak(
        &self,
        _command: &CommandEnvelope<Command>,
    ) -> Result<String, anyhow::Error> {
        Ok("2026/900001".to_string())
    }

    async fn opprett_journalpost(
        &self,
        _command: &CommandEnvelope<Command>,
        _saksnummer: &str,
        _utsending: Option<Utsendingsvalg>,
    ) -> Result<OpprettJournalpostResultat, anyhow::Error> {
        Ok(OpprettJournalpostResultat {
            journalpost_id: 12345,
        })
    }

    async fn legg_til_vedlegg(
        &self,
        _command: &CommandEnvelope<Command>,
        _journalpost_id: i32,
        dokument_ids: Vec<Uuid>,
    ) -> Result<Vec<Option<i32>>, anyhow::Error> {
        Ok(dokument_ids.into_iter().map(|_| Some(777)).collect())
    }

    async fn sett_journalpost_status(
        &self,
        _journalpost_id: i32,
        _status: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn avskriv_journalpost(
        &self,
        _journalpost_id: i32,
        _avskrivingsmaate: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn avslutt_sak(&self, _saksnummer: &str) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

fn nats_image() -> RunnableImage<GenericImage> {
    RunnableImage::from(
        GenericImage::new("nats", "2.10.7")
            .with_exposed_port(4222)
            .with_exposed_port(8222)
            .with_wait_for(WaitFor::message_on_stdout("Server is ready")),
    )
    .with_args(vec![
        "-js".to_string(),
        "-p".to_string(),
        "4222".to_string(),
        "-m".to_string(),
        "8222".to_string(),
    ])
}

async fn setup_postgres() -> Result<(ContainerAsync<Postgres>, PgConnectOptions)> {
    let container = Postgres::default().start().await;
    let port = container.get_host_port_ipv4(5432).await;
    let options = PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username("postgres")
        .password("postgres")
        .database("postgres");
    Ok((container, options))
}

async fn setup_nats() -> Result<(ContainerAsync<GenericImage>, String)> {
    let container = nats_image().start().await;
    let port = container.get_host_port_ipv4(4222).await;
    let nats_url = format!("nats://127.0.0.1:{port}");
    Ok((container, nats_url))
}

async fn apply_migrations(pool: &PgPool) -> Result<()> {
    let migrations = [
        "../src/infrastructure/migrations/20260109144655_id_mapping.up.sql",
        "../src/infrastructure/migrations/20260128133740_add_command_id_to_id_mapping.up.sql",
        "../src/infrastructure/migrations/20260218120000_command_execution_state.up.sql",
    ];

    for path in migrations {
        let sql = tokio::fs::read_to_string(path).await?;
        sqlx::query(&sql).execute(pool).await?;
    }

    Ok(())
}

async fn start_skuffen_runtime(
    nats_url: &str,
    pool: PgPool,
    command_state_repo: Box<dyn CommandStateRepository>,
    arkiv_gateway: Box<dyn ArkivGateway>,
) -> Result<Vec<JoinHandle<()>>> {
    let nats_config = infrastructure::nats::config::NatsConfig {
        server_url: nats_url.to_string(),
        credentials: None,
        require_tls: false,
    };
    let nats_client = NatsClient::connect(&nats_config).await?;

    let id_mapping_repo = PostgresIdMappingRepository::new(pool.clone());

    let command_service = IngestCommandService::new(
        Box::new(id_mapping_repo.clone()),
        Box::new(NatsCommandDispatcher::new(nats_client.clone())),
    );

    let jetstream = jetstream::new(nats_client.inner().clone());
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

    let media_listener = MediaListener::new(nats_client.clone(), media_store.clone());
    let command_listener = CommandListener::new(nats_client.clone(), command_service, media_store);

    let validator_service = ValidateCommandService::new(
        command_state_repo,
        Box::new(id_mapping_repo.clone()),
        Box::new(NatsValidatedCommandDispatcher::new(nats_client.clone())),
        Box::new(NatsCommandStatusPublisher::new(nats_client.clone())),
    );
    let validator_listener = CommandValidationListener::new(nats_client.clone(), validator_service);

    let eksekvering_state_repo = PostgresEksekveringStateRepository::new(pool.clone());
    let eksekvering_service = EksekverKommandoService::new(
        Box::new(eksekvering_state_repo.clone()),
        arkiv_gateway,
        Box::new(NatsEksekveringStatusPublisher::new(nats_client.clone())),
        Box::new(NatsDonePublisher::new(nats_client.clone())),
        Box::new(id_mapping_repo),
    );
    let eksekvering_listener = KommandoEksekveringListener::new(
        nats_client.clone(),
        Box::new(eksekvering_state_repo.clone()),
    );
    let eksekvering_worker = EksekveringWorker::new(
        Box::new(eksekvering_state_repo),
        eksekvering_service,
        "test-worker".to_string(),
        Duration::from_millis(100),
        25,
    );

    let handles = vec![
        tokio::spawn(async move {
            let _ = media_listener.run().await;
        }),
        tokio::spawn(async move {
            let _ = command_listener.run().await;
        }),
        tokio::spawn(async move {
            let _ = validator_listener.run().await;
        }),
        tokio::spawn(async move {
            let _ = eksekvering_listener.run().await;
        }),
        tokio::spawn(async move {
            let _ = eksekvering_worker.run().await;
        }),
    ];

    Ok(handles)
}

async fn publish_media(nats_url: &str, dokument_id: Uuid) -> Result<()> {
    let client = async_nats::connect(nats_url).await?;
    let payload = b"Skuffen testvedlegg".to_vec();
    let metadata = UploadMetadata {
        filename: Some("vedlegg.txt".to_string()),
        content_type: Some("text/plain".to_string()),
    };
    let config = ChunkedUploadConfig::default();
    let chunks = split_payload(&payload, config.chunk_size)?;
    let upload_id = dokument_id.to_string();
    let chunk_count = chunks.len() as u32;
    let total_size = payload.len();

    let inbox = client.new_inbox();
    let mut sub = client.subscribe(inbox.clone()).await?;

    for (index, chunk) in chunks.into_iter().enumerate() {
        let headers =
            build_chunk_headers(&upload_id, index as u32, chunk_count, total_size, &metadata);
        client
            .publish_with_reply_and_headers(
                "arkiv.arkiver.media",
                inbox.clone(),
                headers,
                Bytes::from(chunk),
            )
            .await?;
    }

    let message = tokio::time::timeout(Duration::from_secs(5), sub.next()).await?;
    let message = message.ok_or_else(|| anyhow::anyhow!("Missing media upload response"))?;
    let response_json: serde_json::Value = serde_json::from_slice(&message.payload)?;
    assert_eq!(
        response_json.get("status").and_then(|s| s.as_str()),
        Some("Ok")
    );
    assert_eq!(
        response_json.get("payload").and_then(|p| p.as_str()),
        Some(upload_id.as_str())
    );

    let store = jetstream::new(client)
        .get_object_store("arkiv_media")
        .await?;
    let _ = store.info(&upload_id).await?;
    Ok(())
}

async fn wait_for_done(nats_url: &str, subject: String) -> Result<()> {
    let client = async_nats::connect(nats_url).await?;
    let mut sub = client.subscribe(subject).await?;
    let message = tokio::time::timeout(Duration::from_secs(10), sub.next()).await?;
    if message.is_none() {
        anyhow::bail!("Timed out waiting for done event");
    }
    Ok(())
}

fn build_command_sequence(
    sak_client_reference: Uuid,
    journalpost_client_reference: Uuid,
    dokument_client_reference: Uuid,
    dokument_referanse: Uuid,
    saksbehandler_id: &str,
    saksbehandler_enhet: &str,
    sakstittel: String,
    journalpost_tittel: String,
) -> Vec<CommandEnvelope<Command>> {
    vec![
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettSak(OpprettSak {
                client_reference: sak_client_reference,
                sakstittel: lib_schemas::skuffen::sak::Sakstittel(sakstittel),
                arkivdel: Arkivdel::Tilsynsdivisjonene,
                saksbehandler_id: saksbehandler_id.to_string(),
                saksbehandler_enhet: saksbehandler_enhet.to_string(),
                ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())
                    .unwrap(),
                tilgang: None,
            }),
        },
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
                felles: JournalpostCommon {
                    client_reference: journalpost_client_reference,
                    tittel: journalpost_tittel,
                    dokument_dato: "2025-01-01".to_string(),
                    saksbehandler: saksbehandler_id.to_string(),
                    saksbehandler_enhet: saksbehandler_enhet.to_string(),
                    tilgang: None,
                    dokumenter: vec![Dokument {
                        client_reference: dokument_client_reference,
                        tittel: "Vedlegg".to_string(),
                        filtype: "PDF".to_string(),
                        dokument_referanse,
                    }],
                    sak_key: SakKey::ClientReference(sak_client_reference),
                    kildesystem: None,
                },
            }),
        },
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::AvsluttSak(AvsluttSak {
                sak_key: SakKey::ClientReference(sak_client_reference),
            }),
        },
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_sequence_opprett_internt_notat_avslutt_sak() -> Result<()> {
    let (_postgres, db_options) = setup_postgres().await?;
    let (_nats, nats_url) = setup_nats().await?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(db_options)
        .await?;
    apply_migrations(&pool).await?;

    let _handles = start_skuffen_runtime(
        &nats_url,
        pool.clone(),
        Box::new(FakeCommandStateRepository),
        Box::new(FakeArkivGateway::default()),
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let sak_client_reference = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let dokument_client_reference = Uuid::new_v4();
    let dokument_referanse = Uuid::new_v4();

    publish_media(&nats_url, dokument_referanse).await?;

    let commands = build_command_sequence(
        sak_client_reference,
        journalpost_client_reference,
        dokument_client_reference,
        dokument_referanse,
        "Z12345",
        "42",
        format!("Skuffen E2E test {}", Uuid::new_v4()),
        format!("Internt notat {}", Uuid::new_v4()),
    );

    let payload = serde_json::to_vec(&commands)?;
    let client = async_nats::connect(nats_url.as_str()).await?;
    let response = client.request("arkiv.arkiver", payload.into()).await?;
    let response_json: serde_json::Value = serde_json::from_slice(&response.payload)?;
    assert_eq!(
        response_json.get("status").and_then(|s| s.as_str()),
        Some("Ok")
    );

    for envelope in &commands {
        let subject = format!("arkiv.command.done.journalpost.{}", envelope.command_id);
        if matches!(
            envelope.payload,
            Command::OpprettSak(_) | Command::AvsluttSak(_)
        ) {
            let subject = format!("arkiv.command.done.sak.{}", envelope.command_id);
            wait_for_done(&nats_url, subject).await?;
        } else {
            wait_for_done(&nats_url, subject).await?;
        }
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_sequence_opprett_internt_notat_avslutt_sak_sikri() -> Result<()> {
    if std::env::var("SIKRI_E2E").ok().as_deref() != Some("1") {
        return Ok(());
    }

    let _ = std::env::var("BASE_URL_SIKRI")
        .map_err(|_| anyhow::anyhow!("BASE_URL_SIKRI must be set when SIKRI_E2E=1"))?;
    let _ = std::env::var("APP_APPLICATION__PROJECT_ID")
        .map_err(|_| anyhow::anyhow!("APP_APPLICATION__PROJECT_ID must be set when SIKRI_E2E=1"))?;
    let saksbehandler_id = std::env::var("SIKRI_SAKSBEHANDLER_ID")
        .map_err(|_| anyhow::anyhow!("SIKRI_SAKSBEHANDLER_ID must be set when SIKRI_E2E=1"))?;
    let saksbehandler_enhet = std::env::var("SIKRI_SAKSBEHANDLER_ENHET")
        .map_err(|_| anyhow::anyhow!("SIKRI_SAKSBEHANDLER_ENHET must be set when SIKRI_E2E=1"))?;

    let (_postgres, db_options) = setup_postgres().await?;
    let (_nats, nats_url) = setup_nats().await?;

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(db_options)
        .await?;
    apply_migrations(&pool).await?;

    let _handles = start_skuffen_runtime(
        &nats_url,
        pool.clone(),
        Box::new(SikriCommandStateRepository),
        Box::new(SikriArkivGateway::new()),
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(200)).await;

    let sak_client_reference = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let dokument_client_reference = Uuid::new_v4();
    let dokument_referanse = Uuid::new_v4();

    publish_media(&nats_url, dokument_referanse).await?;

    let commands = build_command_sequence(
        sak_client_reference,
        journalpost_client_reference,
        dokument_client_reference,
        dokument_referanse,
        saksbehandler_id.as_str(),
        saksbehandler_enhet.as_str(),
        format!("Skuffen E2E test {}", Uuid::new_v4()),
        format!("Internt notat {}", Uuid::new_v4()),
    );

    let payload = serde_json::to_vec(&commands)?;
    let client = async_nats::connect(nats_url.as_str()).await?;
    let response = client.request("arkiv.arkiver", payload.into()).await?;
    let response_json: serde_json::Value = serde_json::from_slice(&response.payload)?;
    assert_eq!(
        response_json.get("status").and_then(|s| s.as_str()),
        Some("Ok")
    );

    for envelope in &commands {
        let subject = format!("arkiv.command.done.journalpost.{}", envelope.command_id);
        if matches!(
            envelope.payload,
            Command::OpprettSak(_) | Command::AvsluttSak(_)
        ) {
            let subject = format!("arkiv.command.done.sak.{}", envelope.command_id);
            wait_for_done(&nats_url, subject).await?;
        } else {
            wait_for_done(&nats_url, subject).await?;
        }
    }

    Ok(())
}
