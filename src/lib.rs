use anyhow::Context;
use infrastructure::nats::supervisor::tasknavn;
use tokio::task::JoinSet;

use std::sync::Once;

static STARTUP: Once = Once::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskCriticality {
    Critical,
    Degraded,
}

struct TaskOutcome {
    name: &'static str,
    criticality: TaskCriticality,
    result: anyhow::Result<()>,
}

fn init_crypto() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("install aws-lc-rs provider");
}

fn init_process_once() {
    STARTUP.call_once(|| {
        init_crypto();
        dotenvy::dotenv().ok();
        infrastructure::telemetry::init_observability();
    });
}

pub async fn run() -> anyhow::Result<()> {
    init_process_once();

    let runtime = infrastructure::bootstrap::prepare_runtime().await?;
    let shutdown = tokio_util::sync::CancellationToken::new();

    let helse = runtime.helse.clone();
    // Registreres før noen task er spawnet. Uten dette ville readiness vært
    // sann i vinduet mellom ferdige migrasjoner og første `with_helse`, som er
    // nøyaktig den halvveise tilstanden SKU-0021 skal fjerne. Navnene må
    // stemme med supervisorenes egne — `registrer_task` er idempotent, så
    // supervisoren gjenbruker flagget.
    for navn in infrastructure::nats::supervisor::tasknavn::ALLE {
        helse.registrer_task(navn);
    }

    let query_listener = infrastructure::bootstrap::build_query_listener(
        runtime.nats.clone(),
        runtime.use_fake_sikri,
    );
    let ready_replier = infrastructure::bootstrap::build_ready_replier(runtime.nats.clone());

    let command_listener = infrastructure::bootstrap::build_command_listener(
        runtime.nats.clone(),
        runtime.db_pool.clone(),
        runtime.media_store.clone(),
        helse.clone(),
        shutdown.clone(),
    );
    let validator_listener = infrastructure::bootstrap::build_validator_listener(
        runtime.nats.clone(),
        runtime.db_pool.clone(),
        runtime.use_fake_sikri,
        helse.clone(),
        shutdown.clone(),
    );
    // Må bygges før poolen flyttes inn i execution-wiringen.
    let admin_listener = infrastructure::bootstrap::build_admin_listener(
        runtime.nats.clone(),
        runtime.db_pool.clone(),
        helse.clone(),
        shutdown.clone(),
    );
    let media_nats = runtime.nats.clone();
    let media_store = runtime.media_store.clone();
    let (eksekvering_listener, eksekvering_worker) =
        infrastructure::bootstrap::build_eksekvering_components(
            runtime.nats.clone(),
            runtime.db_pool,
            runtime.media_store,
            runtime.use_fake_sikri,
            helse.clone(),
            shutdown.clone(),
        )?;

    let mut tasks = JoinSet::new();
    spawn_named_task(&mut tasks, "signal_handler", TaskCriticality::Degraded, {
        let shutdown = shutdown.clone();
        async move { infrastructure::bootstrap::vent_paa_nedstengingssignal(shutdown).await }
    });
    spawn_named_task(
        &mut tasks,
        "health_check",
        TaskCriticality::Critical,
        async move {
            match runtime.health_check_handle.await {
                Ok(()) => Ok(()),
                Err(err) => Err(anyhow::anyhow!("health check task join failed: {err}")),
            }
        },
    );
    spawn_named_task(&mut tasks, "query_listener", TaskCriticality::Degraded, {
        let supervisor = supervisor(tasknavn::QUERY_LISTENER, &helse, &shutdown);
        async move {
            supervisor
                .run(|| query_listener.run())
                .await
                .context("query listener failed")
        }
    });
    spawn_named_task(&mut tasks, "ready_replier", TaskCriticality::Degraded, {
        let supervisor = supervisor(tasknavn::READY_REPLIER, &helse, &shutdown);
        async move {
            supervisor
                .run(|| ready_replier.run())
                .await
                .context("ready replier failed")
        }
    });
    spawn_named_task(&mut tasks, "media_listener", TaskCriticality::Critical, {
        let supervisor = supervisor(tasknavn::MEDIA_LISTENER, &helse, &shutdown);
        let shutdown = shutdown.clone();
        async move {
            // `MediaListener::run` konsumerer `self`, så supervisoren må bygge
            // den på nytt hver runde. Både klienten og lageret er billige å
            // klone.
            supervisor
                .run(|| {
                    let nats = media_nats.clone();
                    let store = media_store.clone();
                    let shutdown = shutdown.clone();
                    async move {
                        infrastructure::command::nats::media_listener::MediaListener::new(
                            nats, store,
                        )?
                        .run(shutdown)
                        .await
                        .map_err(anyhow::Error::from)
                    }
                })
                .await
                .context("media listener failed")
        }
    });
    spawn_named_task(
        &mut tasks,
        "command_listener",
        TaskCriticality::Critical,
        async move {
            command_listener
                .run()
                .await
                .context("command listener failed")
        },
    );
    spawn_named_task(
        &mut tasks,
        "validation_listener",
        TaskCriticality::Degraded,
        async move {
            validator_listener
                .run()
                .await
                .context("validation listener failed")
        },
    );
    spawn_named_task(
        &mut tasks,
        "admin_listener",
        TaskCriticality::Degraded,
        async move { admin_listener.run().await.context("admin listener failed") },
    );
    spawn_named_task(
        &mut tasks,
        "execution_listener",
        TaskCriticality::Degraded,
        async move {
            eksekvering_listener
                .run()
                .await
                .context("execution listener failed")
        },
    );
    spawn_named_task(&mut tasks, "execution_worker", TaskCriticality::Degraded, {
        let supervisor = supervisor(tasknavn::EXECUTION_WORKER, &helse, &shutdown);
        async move {
            // Én forbigående DB-feil skal ikke etterlate prosessen uten
            // executor. Restart slipper og gjenerobrer også lederskapet.
            supervisor
                .run(|| eksekvering_worker.run())
                .await
                .context("execution worker failed")
        }
    });

    while let Some(join_result) = tasks.join_next().await {
        let outcome = match join_result {
            Ok(outcome) => outcome,
            Err(err) => TaskOutcome {
                name: "unknown",
                criticality: TaskCriticality::Critical,
                result: Err(anyhow::anyhow!("background task panicked: {err}")),
            },
        };

        // Etter et nedstengingssignal er en avsluttet task normal oppførsel,
        // ikke en degradert eller kritisk feil.
        if shutdown.is_cancelled() {
            if let Err(err) = outcome.result {
                tracing::debug!(task = outcome.name, error = %err, "task stopped during shutdown");
            }
            return avslutt_kontrollert(tasks).await;
        }

        match outcome.result {
            Ok(()) => {
                let message =
                    anyhow::anyhow!("background task {} stopped unexpectedly", outcome.name);
                match outcome.criticality {
                    TaskCriticality::Critical => {
                        tasks.abort_all();
                        while tasks.join_next().await.is_some() {}
                        return Err(message);
                    }
                    TaskCriticality::Degraded => {
                        tracing::error!(
                            task = outcome.name,
                            "degraded background task stopped unexpectedly"
                        );
                    }
                }
            }
            // Tømt restartbudsjett er eneste måten en superviserte task kan
            // returnere `Err`. Fem restarter hjalp ikke, så tasken er død —
            // ikke degradert. Kritikalitet gater ikke dette (SKU-0021 R3).
            Err(err) => {
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}
                return Err(anyhow::anyhow!("task {} failed: {err}", outcome.name));
            }
        }
    }

    Err(anyhow::anyhow!("all background tasks stopped"))
}

/// Cloud Run gir 10 sekunder fra SIGTERM til kill. Vi lar tasks avslutte selv
/// innenfor et kortere vindu, og aborterer resten kontrollert.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(8);

async fn avslutt_kontrollert(mut tasks: JoinSet<TaskOutcome>) -> anyhow::Result<()> {
    let dreneringen = async {
        while let Some(join_result) = tasks.join_next().await {
            match join_result {
                Ok(TaskOutcome {
                    name,
                    result: Err(err),
                    ..
                }) => {
                    tracing::debug!(task = name, error = %err, "task stopped during shutdown");
                }
                Ok(_) => {}
                Err(err) if err.is_cancelled() => {}
                Err(err) => {
                    tracing::warn!(error = %err, "task panicked during shutdown");
                }
            }
        }
    };

    if tokio::time::timeout(SHUTDOWN_GRACE, dreneringen)
        .await
        .is_err()
    {
        tracing::warn!(
            grace_seconds = SHUTDOWN_GRACE.as_secs(),
            "shutdown grace utløp, aborterer gjenstående tasks"
        );
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }

    tracing::info!("skuffen avsluttet kontrollert");
    Ok(())
}

/// Alle arbeidstasks har samme budsjett og samme nedstengingssignal
/// (SKU-0021 R1, R2). `with_shutdown` er ikke kosmetikk: uten den sover en
/// task i backoff med en ukansellerbar sleep når SIGTERM kommer.
fn supervisor(
    name: &'static str,
    helse: &infrastructure::http::helse::Helse,
    shutdown: &tokio_util::sync::CancellationToken,
) -> infrastructure::nats::supervisor::TaskSupervisor {
    infrastructure::nats::supervisor::TaskSupervisor::critical(
        name,
        infrastructure::nats::supervisor::RESTARTBUDSJETT,
    )
    .with_shutdown(shutdown.clone())
    .with_helse(helse)
}

fn spawn_named_task<F>(
    tasks: &mut JoinSet<TaskOutcome>,
    name: &'static str,
    criticality: TaskCriticality,
    future: F,
) where
    F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
{
    tasks.spawn(async move {
        TaskOutcome {
            name,
            criticality,
            result: future.await,
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ferdig_task(name: &'static str) -> TaskOutcome {
        TaskOutcome {
            name,
            criticality: TaskCriticality::Degraded,
            result: Ok(()),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn kontrollert_avslutning_venter_paa_tasks_som_avslutter_selv() {
        let mut tasks = JoinSet::new();
        tasks.spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            ferdig_task("media_listener")
        });
        tasks.spawn(async { ferdig_task("admin_listener") });

        let start = tokio::time::Instant::now();
        avslutt_kontrollert(tasks)
            .await
            .expect("normal nedstenging er ikke en feil");

        assert!(start.elapsed() < SHUTDOWN_GRACE);
    }

    #[tokio::test(start_paused = true)]
    async fn kontrollert_avslutning_aborterer_tasks_som_henger() {
        let mut tasks = JoinSet::new();
        tasks.spawn(async {
            std::future::pending::<()>().await;
            ferdig_task("henger")
        });

        let start = tokio::time::Instant::now();
        avslutt_kontrollert(tasks)
            .await
            .expect("abort etter grace er ikke en feil");

        // Vinduet holder seg innenfor Cloud Runs 10 sekunder.
        assert!(start.elapsed() >= SHUTDOWN_GRACE);
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
    }
}
