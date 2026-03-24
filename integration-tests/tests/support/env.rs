use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::Result;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use testcontainers::{
    core::ContainerPort, runners::AsyncRunner, ContainerAsync, ContainerRequest, GenericImage,
    ImageExt,
};
use testcontainers_modules::postgres::Postgres;

use application::command::ports::command_state_port::ArkivSakTilstandRepository;
use application::command::ports::eksekvering_port::ArkivGateway;
use tokio::sync::{Mutex, OwnedMutexGuard};

use crate::support::nats::{wait_for_nats_ready, wait_for_ready};

const NATS_IMAGE: &str = "nats";
const NATS_TAG: &str = "2.10.7";
const NATS_PORT: u16 = 4222;
const NATS_MONITOR_PORT: u16 = 8222;

pub struct TestEnv {
    pub nats_url: String,
    pub pool: PgPool,
    _postgres: ContainerAsync<Postgres>,
    _nats: ContainerAsync<GenericImage>,
    _skuffen: tokio::task::JoinHandle<anyhow::Result<()>>,
    _guard: OwnedMutexGuard<()>,
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        self._skuffen.abort();
    }
}

fn runtime_lock() -> &'static Arc<Mutex<()>> {
    static LOCK: OnceLock<Arc<Mutex<()>>> = OnceLock::new();
    LOCK.get_or_init(|| Arc::new(Mutex::new(())))
}

fn default_nats_args() -> Vec<String> {
    vec![
        "-js".to_string(),
        "-p".to_string(),
        NATS_PORT.to_string(),
        "-m".to_string(),
        NATS_MONITOR_PORT.to_string(),
    ]
}

fn nats_image() -> ContainerRequest<GenericImage> {
    GenericImage::new(NATS_IMAGE, NATS_TAG)
        .with_exposed_port(ContainerPort::Tcp(NATS_PORT))
        .with_exposed_port(ContainerPort::Tcp(NATS_MONITOR_PORT))
        .with_cmd(default_nats_args())
}

async fn setup_postgres() -> Result<(ContainerAsync<Postgres>, PgConnectOptions)> {
    let container = Postgres::default().start().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let options = PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username("postgres")
        .password("postgres")
        .database("postgres");
    Ok((container, options))
}

async fn setup_nats() -> Result<(ContainerAsync<GenericImage>, String)> {
    let container = nats_image().start().await?;
    let port = container.get_host_port_ipv4(NATS_PORT).await?;
    let nats_url = format!("nats://127.0.0.1:{port}");
    Ok((container, nats_url))
}

fn start_skuffen_process(
    nats_url: &str,
    db_options: &PgConnectOptions,
) -> tokio::task::JoinHandle<anyhow::Result<()>> {
    let base_url_sikri = "http://127.0.0.1:1";
    let project_id = "local-test";
    let otel_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:4317".to_string());
    let db_host = db_options.get_host().to_string();
    let db_port = db_options.get_port().to_string();
    let db_user = db_options.get_username().to_string();
    let db_name = db_options.get_database().unwrap_or("postgres").to_string();
    let nats_url = nats_url.to_string();

    tokio::spawn(async move {
        unsafe {
            std::env::set_var("APP_ENV", "local");
            std::env::set_var("APP_APPLICATION__ENVIRONMENT", "local");
            std::env::set_var("SKUFFEN_FAKE_SIKRI", "1");
            std::env::set_var("DATABASE_HOST", db_host);
            std::env::set_var("DATABASE_PORT", db_port);
            std::env::set_var("DATABASE_USER", db_user);
            std::env::set_var("DATABASE_PASSWORD", "postgres");
            std::env::set_var("DATABASE_NAME", db_name);
            std::env::set_var("NATS_URL", nats_url);
            std::env::set_var("APP_APPLICATION__HOST", "127.0.0.1");
            std::env::set_var("APP_APPLICATION__PORT", "0");
            std::env::set_var("BASE_URL_SIKRI", base_url_sikri);
            std::env::set_var("APP_APPLICATION__PROJECT_ID", project_id);
            std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", otel_endpoint);
            std::env::remove_var("APP_NATS_CREDENTIALS");
            std::env::set_var("APP_NATS_CREDENTIALS", " ");
        }

        skuffen::run().await
    })
}

pub async fn start_runtime(
    _command_state_repo: Box<dyn ArkivSakTilstandRepository>,
    _arkiv_gateway: Box<dyn ArkivGateway>,
    _query_repos: Option<Arc<dyn std::any::Any + Send + Sync>>,
) -> Result<TestEnv> {
    let guard = runtime_lock().clone().lock_owned().await;

    eprintln!("start_runtime: starting postgres container");
    let (_postgres, db_options) = setup_postgres().await?;
    eprintln!(
        "start_runtime: postgres ready on {}:{}",
        db_options.get_host(),
        db_options.get_port()
    );
    eprintln!("start_runtime: starting nats container");
    let (_nats, nats_url) = setup_nats().await?;
    eprintln!("start_runtime: waiting for nats ready");
    wait_for_nats_ready(&nats_url, Duration::from_secs(15)).await?;
    eprintln!("start_runtime: nats ready at {}", nats_url);

    eprintln!("start_runtime: connecting postgres pool");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(db_options.clone())
        .await
        .map_err(|err| anyhow::anyhow!("connect test postgres: {err}"))?;
    let pool = pool.clone();
    eprintln!("start_runtime: spawning skuffen process");
    let skuffen = start_skuffen_process(&nats_url, &db_options);
    eprintln!("start_runtime: waiting for skuffen ready");
    wait_for_skuffen_ready(&nats_url).await?;
    eprintln!("start_runtime: skuffen ready");

    Ok(TestEnv {
        nats_url,
        pool,
        _guard: guard,
        _postgres,
        _nats,
        _skuffen: skuffen,
    })
}

async fn wait_for_skuffen_ready(nats_url: &str) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        match wait_for_ready(nats_url).await {
            Ok(()) => return Ok(()),
            Err(err) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(err);
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}
