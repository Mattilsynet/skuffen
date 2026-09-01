use anyhow::{Context, ensure};
use application::admin::services::admin_read_service::AdminReadService;
use application::command::ports::command_state_port::ArkivSakTilstandRepository;
use application::command::ports::dokument_renderer_port::{
    DokumentRenderer, IkkeKonfigurertDokumentRenderer,
};
use application::command::ports::eksekvering_port::ArkivGateway;
use application::query::services::hent_journalpost::HentJournalpostService;
use application::query::services::hent_sak::HentSakService;
use async_trait::async_trait;
use lib_nats::jetstream;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::admin::adapter::postgres_admin_read_repository::PostgresAdminReadRepository;
use crate::admin::nats::listener::{AdminListener, NatsAdminTransport};
use crate::command::adapter::fake_arkiv_gateway::FakeArkivGateway;
use crate::command::adapter::fake_command_state_repo::FakeArkivSakTilstandRepository;
use crate::command::adapter::html2pdf_renderer_adapter::{
    GcpIdTokenProvider, Html2PdfRendererAdapter,
};
use crate::command::adapter::nats_ingested_publisher::NatsCommandDispatcher;
use crate::command::adapter::nats_status_publisher::NatsStatusPublisher;
use crate::command::adapter::nats_validated_publisher::NatsValidatedCommandDispatcher;
use crate::command::adapter::postgres_command_repository::PostgresCommandRepository;
use crate::command::adapter::postgres_entitet_repository::PostgresEntitetRepository;
use crate::command::adapter::postgres_fakta_repository::PostgresFaktaRepository;
use crate::command::adapter::postgres_operasjon_repository::PostgresOperasjonRepository;
use crate::command::adapter::sikri_arkiv_gateway::SikriArkivGateway;
use crate::command::adapter::sikri_command_state_repo::SikriCommandStateRepository;
use crate::command::media::ObjectStoreMediaStore;
use crate::command::nats::command_listener::CommandListener;
use crate::command::nats::dekomponering_listener::DekomponeringListener;
use crate::command::nats::validation_listener::CommandValidationListener;
use crate::http::health_check::health_check;
use crate::nats::client::NatsClient;
use crate::nats::jetstream_setup::ensure_media_object_store;
use crate::nats::setup::setup_nats;
use crate::query::adapter::fake_journalpost_repository::FakeJournalpostRepository;
use crate::query::adapter::fake_sak_repository::FakeSakRepository;
use crate::query::adapter::hent_sak::SikriRepository;
use crate::query::adapter::not_implemented_journalpost_repository::NotImplementedJournalpostRepository;
use crate::query::mapping::lookup::entitet_queries;
use crate::query::nats::listener::{
    BRUKER_MT_ENHETER_SUBJECT, BrukerMtEnheterNotImplementedUseCase, HENT_JOURNALPOST_SUBJECT,
    HENT_SAK_SUBJECT, NatsReplier, UseCase,
};
use crate::query::nats::query_listener::QueryListener;

pub struct RuntimeDeps {
    pub nats: NatsClient,
    pub health_check_handle: JoinHandle<()>,
    pub db_pool: lib_sql::database_config::DbPool,
    pub media_store: std::sync::Arc<ObjectStoreMediaStore>,
    pub use_fake_sikri: bool,
}

pub async fn prepare_runtime() -> anyhow::Result<RuntimeDeps> {
    let nats = setup_nats().await?;
    let health_check_handle = health_check().await?;
    let use_fake_sikri = use_fake_sikri()?;

    let db_pool = crate::database::setup::setup_database().await?;
    crate::database::setup::run_migrations(&db_pool).await?;

    entitet_queries::init_entitet_repo(std::sync::Arc::new(PostgresEntitetRepository::new(
        db_pool.clone(),
    )));

    let media_store = setup_media_store(nats.clone()).await?;

    Ok(RuntimeDeps {
        nats,
        health_check_handle,
        db_pool,
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
    // Fake journalpost-data skal aldri nå ekte klienter. Inntil ekte backing
    // finnes returnerer produksjonsadapteren en tydelig feil (SKU-0008 R7).
    let hent_journalpost_uc = if use_fake_sikri {
        HentJournalpostService::new(Box::new(FakeJournalpostRepository::new()))
    } else {
        HentJournalpostService::new(Box::new(NotImplementedJournalpostRepository::new()))
    };
    let hent_sak_replier = NatsReplier::new(nats.clone(), HENT_SAK_SUBJECT, Box::new(hent_sak_uc));
    let hent_journalpost_replier = NatsReplier::new(
        nats.clone(),
        HENT_JOURNALPOST_SUBJECT,
        Box::new(hent_journalpost_uc),
    );
    let bruker_mt_enheter_replier = NatsReplier::new(
        nats,
        BRUKER_MT_ENHETER_SUBJECT,
        Box::new(BrukerMtEnheterNotImplementedUseCase),
    );

    QueryListener::new(
        hent_sak_replier,
        hent_journalpost_replier,
        bruker_mt_enheter_replier,
    )
}

/// Cloud Run sender SIGTERM og dreper containeren 10 sekunder senere.
pub async fn vent_paa_nedstengingssignal(
    shutdown: tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;

    let signalnavn = tokio::select! {
        _ = terminate.recv() => "SIGTERM",
        _ = interrupt.recv() => "SIGINT",
    };

    info!(signal = signalnavn, "nedstenging signalisert");
    shutdown.cancel();
    // Cloud Run river containeren kort tid etter signalet. Uten en eksplisitt
    // tømming går siste batch med spans tapt — som regel nettopp den som
    // forklarer hvorfor tjenesten stoppet.
    crate::telemetry::shutdown_telemetry();
    Ok(())
}

pub fn build_ready_replier(nats: NatsClient) -> NatsReplier<String, String> {
    NatsReplier::<String, String>::new(nats, "skuffen.ready", Box::new(ReadyUseCase))
}

pub fn build_command_listener(
    nats: NatsClient,
    db_pool: lib_sql::database_config::DbPool,
    media_store: std::sync::Arc<ObjectStoreMediaStore>,
) -> CommandListener {
    let command_service = application::command::services::ingest_command::IngestCommandService::new(
        Box::new(PostgresCommandRepository::new(db_pool.clone())),
        Box::new(PostgresEntitetRepository::new(db_pool)),
        Box::new(NatsCommandDispatcher::new(nats.clone())),
        Box::new(NatsStatusPublisher::new(nats.clone())),
    );

    CommandListener::new(nats, command_service, media_store)
}

/// Admin read deler ett repository mellom de to use casene.
pub fn build_admin_listener(
    nats: NatsClient,
    db_pool: lib_sql::database_config::DbPool,
    shutdown: tokio_util::sync::CancellationToken,
) -> AdminListener {
    let repository = std::sync::Arc::new(PostgresAdminReadRepository::new(db_pool));
    let service = std::sync::Arc::new(AdminReadService::new(repository));

    AdminListener::new(
        std::sync::Arc::new(NatsAdminTransport::new(nats)),
        service,
        shutdown,
    )
}

pub fn build_validator_listener(
    nats: NatsClient,
    db_pool: lib_sql::database_config::DbPool,
    use_fake_sikri: bool,
) -> CommandValidationListener {
    let validator_service =
        application::command::services::validate_command::ValidateCommandService::new(
            command_state_repository(use_fake_sikri),
            Box::new(PostgresEntitetRepository::new(db_pool)),
            Box::new(NatsValidatedCommandDispatcher::new(nats.clone())),
            Box::new(NatsStatusPublisher::new(nats.clone())),
        );

    CommandValidationListener::new(nats, validator_service)
}

pub fn build_eksekvering_components(
    nats: NatsClient,
    db_pool: lib_sql::database_config::DbPool,
    media_store: std::sync::Arc<ObjectStoreMediaStore>,
    use_fake_sikri: bool,
    shutdown: tokio_util::sync::CancellationToken,
) -> anyhow::Result<(
    DekomponeringListener,
    application::command::services::operasjon_worker::OperasjonWorker,
)> {
    use application::command::services::{
        dekomponer_command::DekomponerCommandService,
        eksekver_operasjon::EksekverOperasjonService,
        evaluer_operasjoner::EvaluerOperasjonerService,
        operasjon_worker::{OperasjonWorker, WorkerInnstillinger},
    };

    let operasjon = std::sync::Arc::new(PostgresOperasjonRepository::new(db_pool.clone()));
    let fakta = std::sync::Arc::new(PostgresFaktaRepository::new(db_pool.clone()));
    let publisher = std::sync::Arc::new(NatsStatusPublisher::new(nats.clone()));

    let dekomponer_service = DekomponerCommandService::new(
        Box::new(PostgresEntitetRepository::new(db_pool.clone())),
        Box::new(PostgresOperasjonRepository::new(db_pool.clone())),
        Box::new(NatsStatusPublisher::new(nats.clone())),
    );

    let executor = EksekverOperasjonService::new(
        Box::new(PostgresOperasjonRepository::new(db_pool.clone())),
        Box::new(PostgresFaktaRepository::new(db_pool)),
        arkiv_gateway(use_fake_sikri, media_store.clone()),
        Box::new(
            crate::command::adapter::media_render_operasjon::MediaRenderOperasjon::new(
                media_store,
                dokument_renderer()?,
            ),
        ),
        Box::new(NatsStatusPublisher::new(nats.clone())),
        EXECUTOR_ID,
        avvent_journalfort_poll_intervall(use_fake_sikri),
    );

    let evaluator = EvaluerOperasjonerService::new(operasjon.clone(), fakta, EVALUERINGSGRENSE);

    let worker = OperasjonWorker::new(
        executor,
        evaluator,
        operasjon,
        publisher,
        EXECUTOR_ID,
        WorkerInnstillinger::default(),
        shutdown,
    );

    Ok((DekomponeringListener::new(nats, dekomponer_service), worker))
}

const EXECUTOR_ID: &str = "worker-1";
const EVALUERINGSGRENSE: i64 = 200;
/// RPA journalfører i begge utgående løp, med observert latens på en halv til
/// én time. Intervallet skal tunes mot faktisk RPA-latens.
///
/// Mot fake-arkivet finnes ingen robot å vente på, så der poller vi raskt.
fn avvent_journalfort_poll_intervall(use_fake_sikri: bool) -> std::time::Duration {
    if use_fake_sikri {
        std::time::Duration::from_millis(200)
    } else {
        std::time::Duration::from_secs(60 * 60)
    }
}

fn use_fake_sikri() -> anyhow::Result<bool> {
    let requested = std::env::var("SKUFFEN_FAKE_SIKRI")
        .map(|value| value == "1")
        .unwrap_or(false);

    // Fake Sikri/arkiv skal ALDRI kunne aktiveres utenfor eksplisitt godkjente
    // ikke-prod-miljøer. Feilkonfig i prod skal stoppe oppstart, ikke gi falske
    // OK-svar mot ekte klienter.
    if requested && !fake_adaptere_tillatt() {
        anyhow::bail!("SKUFFEN_FAKE_SIKRI=1 er kun tillatt når APP_ENV er local, dev eller test");
    }

    Ok(requested)
}

/// Fake-adaptere er kun tillatt i eksplisitt ikke-prod-miljøer.
fn fake_adaptere_tillatt() -> bool {
    fake_adaptere_tillatt_for(std::env::var("APP_ENV").ok().as_deref())
}

fn fake_adaptere_tillatt_for(app_env: Option<&str>) -> bool {
    app_env
        .map(|value| {
            let env = value.trim().to_ascii_lowercase();
            matches!(env.as_str(), "local" | "dev" | "test")
        })
        .unwrap_or(false)
}

fn dokument_renderer() -> anyhow::Result<Box<dyn DokumentRenderer>> {
    let endpoint = match optional_env("SKUFFEN_HTML2PDF_RENDERER_ENDPOINT") {
        Some(endpoint) => endpoint,
        None => {
            warn!(
                renderer_mode = "unconfigured",
                "HTML-to-PDF renderer is not configured"
            );
            return Ok(Box::new(IkkeKonfigurertDokumentRenderer));
        }
    };
    validate_renderer_endpoint(&endpoint)?;

    info!(
        renderer_mode = "enabled",
        "HTML-to-PDF renderer is configured"
    );

    Ok(Box::new(Html2PdfRendererAdapter::new(
        endpoint.clone(),
        endpoint,
        Box::new(GcpIdTokenProvider),
    )))
}

fn validate_renderer_endpoint(endpoint: &str) -> anyhow::Result<()> {
    let parsed = reqwest::Url::parse(endpoint)
        .context("SKUFFEN_HTML2PDF_RENDERER_ENDPOINT is not a valid URL")?;
    ensure!(
        parsed.scheme() == "https" || (parsed.scheme() == "http" && is_local_env()),
        "SKUFFEN_HTML2PDF_RENDERER_ENDPOINT must use https outside APP_ENV=local"
    );
    ensure!(
        parsed.username().is_empty() && parsed.password().is_none(),
        "SKUFFEN_HTML2PDF_RENDERER_ENDPOINT must not contain credentials"
    );
    ensure!(
        parsed.path().is_empty() || parsed.path() == "/",
        "SKUFFEN_HTML2PDF_RENDERER_ENDPOINT must be the renderer base URL without /render"
    );
    ensure!(
        parsed.query().is_none() && parsed.fragment().is_none(),
        "SKUFFEN_HTML2PDF_RENDERER_ENDPOINT must not contain query or fragment"
    );
    Ok(())
}

fn is_local_env() -> bool {
    std::env::var("APP_ENV")
        .map(|value| value.trim().eq_ignore_ascii_case("local"))
        .unwrap_or(false)
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

async fn setup_media_store(
    nats: NatsClient,
) -> anyhow::Result<std::sync::Arc<ObjectStoreMediaStore>> {
    let jetstream = jetstream::new(nats.inner().clone());
    let store = ensure_media_object_store(&jetstream, nats.jetstream_replicas()).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_adaptere_tillatt_kun_i_ikke_prod_miljoer() {
        assert!(fake_adaptere_tillatt_for(Some("local")));
        assert!(fake_adaptere_tillatt_for(Some("dev")));
        assert!(fake_adaptere_tillatt_for(Some("test")));
        assert!(fake_adaptere_tillatt_for(Some("TEST")));
        assert!(fake_adaptere_tillatt_for(Some("  dev  ")));

        assert!(!fake_adaptere_tillatt_for(Some("prod")));
        assert!(!fake_adaptere_tillatt_for(Some("production")));
        assert!(!fake_adaptere_tillatt_for(Some("")));
        assert!(!fake_adaptere_tillatt_for(None));
    }
}
