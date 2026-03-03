use std::sync::Arc;

use anyhow::Result;
use std::time::Duration;
use uuid::Uuid;

use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::sak::Saksnummer as DtoSaksnummer;

use support::{
    fetch_dokument_state, fetch_journalpost_state, fetch_sak_state, hent_journalpost_via_nats,
    hent_sak_via_nats, insert_arkiv_id_mapping, insert_id_mapping, publish_media,
    send_command_batch, wait_for_command_execution_all, wait_for_status_events, CommandScenario,
    FakeArkivGateway, FakeArkivGatewayState, FakeCommandStateRepository, TestEnv,
};

mod support;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_sequence_opprett_internt_notat_avslutt_sak() -> Result<()> {
    let arkiv_state = Arc::new(FakeArkivGatewayState::new());
    let arkiv_gateway = FakeArkivGateway::new(arkiv_state.clone());
    let env = support::start_runtime(
        Box::new(FakeCommandStateRepository),
        Box::new(arkiv_gateway),
        None,
    )
    .await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    let commands = scenario.build_sequence(
        "Z12345",
        "42",
        format!("Skuffen E2E test {}", Uuid::new_v4()),
        format!("Internt notat {}", Uuid::new_v4()),
    );
    send_command_batch(&env.nats_url, &commands).await?;
    let _ = wait_for_status_events(
        &env.nats_url,
        commands.iter().map(|command| command.command_id),
        Duration::from_secs(20),
    )
    .await?;
    wait_for_command_execution_all(
        &env.pool,
        commands.iter().map(|command| command.command_id),
        Duration::from_secs(20),
    )
    .await?;

    let sak_state = fetch_sak_state(&env.pool, scenario.sak_client_reference).await?;
    assert!(sak_state.is_some());

    let journalpost_state =
        fetch_journalpost_state(&env.pool, scenario.journalpost_internt_client_reference).await?;
    assert!(journalpost_state.is_some());

    let dokument_state =
        fetch_dokument_state(&env.pool, scenario.dokument_client_reference).await?;
    assert!(dokument_state.is_some());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_sequence_inngaende_journalpost_flow() -> Result<()> {
    let arkiv_gateway = FakeArkivGateway::new(Arc::new(FakeArkivGatewayState::new()));
    let env = support::start_runtime(
        Box::new(FakeCommandStateRepository),
        Box::new(arkiv_gateway),
        None,
    )
    .await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    let saksnummer = "2026/100001";
    let commands = vec![scenario.opprett_inngaende(
        "Z99999",
        "42",
        DtoSakKey::ArkivId(DtoSaksnummer::new(saksnummer)?),
        "Inngaaende",
    )];
    insert_arkiv_id_mapping(&env.pool, scenario.sak_skuffen_id, "sak", saksnummer).await?;
    send_command_batch(&env.nats_url, &commands).await?;
    let _ = wait_for_status_events(
        &env.nats_url,
        commands.iter().map(|command| command.command_id),
        Duration::from_secs(20),
    )
    .await?;
    wait_for_command_execution_all(
        &env.pool,
        commands.iter().map(|command| command.command_id),
        Duration::from_secs(20),
    )
    .await?;

    let journalpost_state =
        fetch_journalpost_state(&env.pool, scenario.journalpost_inngaende_client_reference).await?;
    let journalpost_state = journalpost_state.expect("journalpost state should exist");
    assert!(journalpost_state.journalfoert);
    assert!(journalpost_state.avskrevet);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_sequence_utgaaende_journalpost_flow() -> Result<()> {
    let arkiv_gateway = FakeArkivGateway::new(Arc::new(FakeArkivGatewayState::new()));
    let env = support::start_runtime(
        Box::new(FakeCommandStateRepository),
        Box::new(arkiv_gateway),
        None,
    )
    .await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    let saksnummer = "2026/100002";
    let commands = vec![scenario.opprett_utgaaende(
        "Z99999",
        "42",
        DtoSakKey::ArkivId(DtoSaksnummer::new(saksnummer)?),
        "Utgaaende",
    )];
    insert_arkiv_id_mapping(&env.pool, scenario.sak_skuffen_id, "sak", saksnummer).await?;
    send_command_batch(&env.nats_url, &commands).await?;
    let _ = wait_for_status_events(
        &env.nats_url,
        commands.iter().map(|command| command.command_id),
        Duration::from_secs(20),
    )
    .await?;
    wait_for_command_execution_all(
        &env.pool,
        commands.iter().map(|command| command.command_id),
        Duration::from_secs(20),
    )
    .await?;

    let journalpost_state =
        fetch_journalpost_state(&env.pool, scenario.journalpost_utgaaende_client_reference).await?;
    let journalpost_state = journalpost_state.expect("journalpost state should exist");
    assert!(journalpost_state.journalfoert);
    assert!(!journalpost_state.avskrevet);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn query_hent_sak_via_nats_uses_id_mapping() -> Result<()> {
    let arkiv_state = Arc::new(FakeArkivGatewayState::new());
    let arkiv_gateway = FakeArkivGateway::new(arkiv_state);

    let env = support::start_runtime(
        Box::new(FakeCommandStateRepository),
        Box::new(arkiv_gateway),
        None,
    )
    .await?;

    let scenario = CommandScenario::new();
    let skuffen_id = scenario.sak_skuffen_id;
    let arkiv_id = "2026/123456";

    insert_id_mapping(
        &env.pool,
        skuffen_id,
        "sak",
        scenario.sak_client_reference,
        Some(arkiv_id),
    )
    .await?;

    let response = hent_sak_via_nats(&env.nats_url, skuffen_id).await?;
    assert_eq!(response.get("status").and_then(|s| s.as_str()), Some("Ok"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn query_hent_journalpost_via_nats() -> Result<()> {
    let arkiv_state = Arc::new(FakeArkivGatewayState::new());
    let arkiv_gateway = FakeArkivGateway::new(arkiv_state);

    let env: TestEnv = support::start_runtime(
        Box::new(FakeCommandStateRepository),
        Box::new(arkiv_gateway),
        None,
    )
    .await?;

    let scenario = CommandScenario::new();
    let response =
        hent_journalpost_via_nats(&env.nats_url, scenario.journalpost_internt_client_reference)
            .await?;
    assert_eq!(response.get("status").and_then(|s| s.as_str()), Some("Ok"));
    Ok(())
}
