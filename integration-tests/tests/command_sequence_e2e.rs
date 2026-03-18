use std::sync::Arc;

use anyhow::Result;
use std::time::Duration;
use uuid::Uuid;

use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn avslutt_sak_uten_journalposter_er_tillatt() -> Result<()> {
    let arkiv_gateway = FakeArkivGateway::new(Arc::new(FakeArkivGatewayState::new()));
    let env = support::start_runtime(
        Box::new(FakeCommandStateRepository),
        Box::new(arkiv_gateway),
        None,
    )
    .await?;

    let sak_client_reference = Uuid::new_v4();
    let commands = vec![
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettSak(OpprettSak {
                client_reference: sak_client_reference,
                sakstittel: lib_schemas::skuffen::sak::Sakstittel(format!(
                    "Skuffen E2E avslutt uten journalposter {}",
                    Uuid::new_v4()
                )),
                arkivdel: Arkivdel::Tilsynsdivisjonene,
                saksbehandler_id: "Z12345".to_string(),
                saksbehandler_enhet: "42".to_string(),
                ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())?,
                tilgang: None,
            }),
        },
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::AvsluttSak(AvsluttSak {
                sak_key: DtoSakKey::ClientReference(sak_client_reference),
            }),
        },
    ];

    send_command_batch(&env.nats_url, &commands).await?;
    wait_for_command_execution_all(
        &env.pool,
        commands.iter().map(|command| command.command_id),
        Duration::from_secs(20),
    )
    .await?;

    let sak_state = fetch_sak_state(&env.pool, sak_client_reference)
        .await?
        .expect("sak state should exist");
    assert_eq!(sak_state.status, "A");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn avslutt_sak_blokkeres_nar_journalpost_ikke_er_ok() -> Result<()> {
    let arkiv_gateway = FakeArkivGateway::new(Arc::new(FakeArkivGatewayState::new()));
    let env = support::start_runtime(
        Box::new(FakeCommandStateRepository),
        Box::new(arkiv_gateway),
        None,
    )
    .await?;

    let sak_client_reference = Uuid::new_v4();
    let opprett_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel(format!(
                "Skuffen E2E blokkert med ventende journalpost {}",
                Uuid::new_v4()
            )),
            arkivdel: Arkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())?,
            tilgang: None,
        }),
    };

    send_command_batch(&env.nats_url, std::slice::from_ref(&opprett_sak)).await?;
    wait_for_command_execution_all(&env.pool, [opprett_sak.command_id], Duration::from_secs(20))
        .await?;

    let journalpost_id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO journalpost_state (
            journalpost_id,
            sak_id,
            journalpostnummer,
            journalposttype,
            med_utsending,
            journalfoert,
            avskrevet,
            ekspedert,
            har_feilede_dokumenter
        ) VALUES ($1, $2, NULL, 'X', false, false, false, false, false)
        "#,
    )
    .bind(journalpost_id)
    .bind(sak_client_reference)
    .execute(&env.pool)
    .await?;

    let avslutt_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::AvsluttSak(AvsluttSak {
            sak_key: DtoSakKey::ClientReference(sak_client_reference),
        }),
    };
    send_command_batch(&env.nats_url, std::slice::from_ref(&avslutt_sak)).await?;
    wait_for_command_execution_all(&env.pool, [avslutt_sak.command_id], Duration::from_secs(20))
        .await?;

    let avslutt_status: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT status
        FROM command_execution
        WHERE command_id = $1
        "#,
    )
    .bind(avslutt_sak.command_id)
    .fetch_optional(&env.pool)
    .await?;
    assert_eq!(avslutt_status.map(|(s,)| s), Some("blocked".to_string()));

    let sak_state = fetch_sak_state(&env.pool, sak_client_reference)
        .await?
        .expect("sak state should exist");
    assert_eq!(sak_state.status, "B");

    Ok(())
}
