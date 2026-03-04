use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::PgPool;
use testcontainers::{
    core::ContainerPort,
    runners::AsyncRunner,
    ContainerAsync, ContainerRequest, GenericImage, ImageExt,
};
use testcontainers_modules::postgres::Postgres;

use application::command::ports::command_state_port::CommandStateRepository;
use application::command::ports::eksekvering_port::ArkivGateway;

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
    _skuffen: tokio::process::Child,
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

async fn start_skuffen_process(
    nats_url: &str,
    db_options: &PgConnectOptions,
) -> Result<tokio::process::Child> {
    let base_url_sikri = "http://127.0.0.1:1";
    let project_id = "local-test";
    let otel_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://127.0.0.1:4317".to_string());
    let binary = resolve_binary_path();
    let mut cmd = tokio::process::Command::new(binary);
    cmd.current_dir(workspace_root());
    cmd.env("APP_ENV", "local");
    cmd.env("APP_APPLICATION__ENVIRONMENT", "local");
    cmd.env("SKUFFEN_FAKE_SIKRI", "1");
    cmd.env("DATABASE_HOST", db_options.get_host());
    cmd.env("DATABASE_PORT", db_options.get_port().to_string());
    cmd.env("DATABASE_USER", db_options.get_username());
    cmd.env("DATABASE_PASSWORD", "postgres");
    cmd.env(
        "DATABASE_NAME",
        db_options.get_database().unwrap_or("postgres"),
    );
    cmd.env("NATS_URL", nats_url);
    cmd.env("APP_APPLICATION__HOST", "127.0.0.1");
    cmd.env("APP_APPLICATION__PORT", "0");
    cmd.env("BASE_URL_SIKRI", base_url_sikri);
    cmd.env("APP_APPLICATION__PROJECT_ID", project_id);
    cmd.env("OTEL_EXPORTER_OTLP_ENDPOINT", otel_endpoint);
    cmd.env_remove("APP_NATS_CREDENTIALS");
    cmd.env("APP_NATS_CREDENTIALS", " ");
    cmd.kill_on_drop(true);
    let child = cmd
        .spawn()
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    Ok(child)
}

pub async fn start_runtime(
    _command_state_repo: Box<dyn CommandStateRepository>,
    _arkiv_gateway: Box<dyn ArkivGateway>,
    _query_repos: Option<Arc<dyn std::any::Any + Send + Sync>>,
) -> Result<TestEnv> {
    let _ =
        std::env::var("SKUFFEN_BIN").map(|path| tracing::info!("SKUFFEN_BIN override: {}", path));
    let binary_path = resolve_binary_path();
    eprintln!("start_runtime: skuffen binary at {}", binary_path.display());
    tracing::info!("Resolved skuffen binary: {}", binary_path.display());
    match tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if tokio::fs::metadata(&binary_path).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    {
        Ok(()) => {}
        Err(_) => return Err(anyhow::anyhow!("Timed out waiting for skuffen binary")),
    }

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
    let skuffen = start_skuffen_process(&nats_url, &db_options).await?;
    eprintln!("start_runtime: waiting for skuffen ready");
    wait_for_skuffen_ready(&nats_url).await?;
    eprintln!("start_runtime: skuffen ready");

    Ok(TestEnv {
        nats_url,
        pool,
        _postgres,
        _nats,
        _skuffen: skuffen,
    })
}

fn resolve_binary_path() -> PathBuf {
    let workspace_root = workspace_root();
    let default_path = workspace_root.join("target").join("debug").join("skuffen");
    match std::env::var("SKUFFEN_BIN") {
        Ok(value) => {
            let path = PathBuf::from(value);
            if path.is_relative() {
                workspace_root.join(path)
            } else {
                path
            }
        }
        Err(_) => default_path,
    }
}

fn workspace_root() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let manifest_path = Path::new(&manifest_dir);
    manifest_path
        .parent()
        .map(|parent| parent.to_path_buf())
        .unwrap_or_else(|| manifest_path.to_path_buf())
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
