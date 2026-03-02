use anyhow::Result;
use std::time::Duration;

use uuid::Uuid;

use infrastructure::command::adapter::sikri_arkiv_gateway::SikriArkivGateway;

use crate::support::{
    publish_media, send_command_batch, wait_for_command_execution_all, CommandScenario,
    FakeCommandStateRepository, TestEnv,
};

pub async fn run_sikri_sequence() -> Result<()> {
    let _ = std::env::var("BASE_URL_SIKRI")
        .map_err(|_| anyhow::anyhow!("BASE_URL_SIKRI must be set when SIKRI_E2E=1"))?;
    let _ = std::env::var("APP_APPLICATION__PROJECT_ID")
        .map_err(|_| anyhow::anyhow!("APP_APPLICATION__PROJECT_ID must be set when SIKRI_E2E=1"))?;
    let saksbehandler_id = std::env::var("SIKRI_SAKSBEHANDLER_ID")
        .map_err(|_| anyhow::anyhow!("SIKRI_SAKSBEHANDLER_ID must be set when SIKRI_E2E=1"))?;
    let saksbehandler_enhet = std::env::var("SIKRI_SAKSBEHANDLER_ENHET")
        .map_err(|_| anyhow::anyhow!("SIKRI_SAKSBEHANDLER_ENHET must be set when SIKRI_E2E=1"))?;

    let env: TestEnv = crate::support::start_runtime(
        Box::new(FakeCommandStateRepository),
        Box::new(SikriArkivGateway::new()),
        None,
    )
    .await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    let commands = scenario.build_sequence(
        saksbehandler_id.as_str(),
        saksbehandler_enhet.as_str(),
        format!("Skuffen E2E test {}", Uuid::new_v4()),
        format!("Internt notat {}", Uuid::new_v4()),
    );

    send_command_batch(&env.nats_url, &commands).await?;
    wait_for_command_execution_all(
        &env.pool,
        commands.iter().map(|command| command.command_id),
        Duration::from_secs(20),
    )
    .await?;
    Ok(())
}
