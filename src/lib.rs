use anyhow::Context;
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

    let query_listener = infrastructure::bootstrap::build_query_listener(
        runtime.nats.clone(),
        runtime.use_fake_sikri,
    );
    let ready_replier = infrastructure::bootstrap::build_ready_replier(runtime.nats.clone());

    let media_listener = infrastructure::command::nats::media_listener::MediaListener::new(
        runtime.nats.clone(),
        runtime.media_store.clone(),
    );
    let command_listener = infrastructure::bootstrap::build_command_listener(
        runtime.nats.clone(),
        runtime.db_pool.clone(),
        runtime.media_store.clone(),
    );
    let validator_listener = infrastructure::bootstrap::build_validator_listener(
        runtime.nats.clone(),
        runtime.db_pool.clone(),
        runtime.use_fake_sikri,
    );
    let (eksekvering_listener, eksekvering_worker) =
        infrastructure::bootstrap::build_eksekvering_components(
            runtime.nats.clone(),
            runtime.db_pool,
            runtime.media_store,
            runtime.use_fake_sikri,
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
    spawn_named_task(
        &mut tasks,
        "query_listener",
        TaskCriticality::Degraded,
        async move { query_listener.run().await.context("query listener failed") },
    );
    spawn_named_task(
        &mut tasks,
        "ready_replier",
        TaskCriticality::Degraded,
        async move { ready_replier.run().await.context("ready replier failed") },
    );
    spawn_named_task(
        &mut tasks,
        "media_listener",
        TaskCriticality::Critical,
        async move { media_listener.run().await.context("media listener failed") },
    );
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
        let shutdown = shutdown.clone();
        async move {
            // Degraded betyr «prøv igjen». Én forbigående DB-feil skal ikke
            // etterlate prosessen uten executor for alltid — den skal restarte
            // med backoff, og da også slippe og gjenerobre lederskapet.
            infrastructure::nats::supervisor::TaskSupervisor::background("execution_worker")
                .with_shutdown(shutdown)
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
            Err(err) => match outcome.criticality {
                TaskCriticality::Critical => {
                    tasks.abort_all();
                    while tasks.join_next().await.is_some() {}
                    return Err(anyhow::anyhow!(
                        "critical task {} failed: {err}",
                        outcome.name
                    ));
                }
                TaskCriticality::Degraded => {
                    tracing::error!(task = outcome.name, error = %err, "degraded background task failed");
                }
            },
        }
    }

    Err(anyhow::anyhow!("all background tasks stopped"))
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
