use anyhow::Result;
use std::time::Duration;
use uuid::Uuid;

use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::sak::Saksnummer as DtoSaksnummer;
use lib_schemas::skuffen::status::{SkuffenStatus, SkuffenStatusEventV1, SkuffenStatusPhase};

use support::{
    CommandScenario, extract_saksnummer, hent_bruker_mt_enheter_via_nats,
    hent_journalpost_via_nats, hent_sak_via_nats_by_arkiv_id, publish_media, send_command_batch,
    wait_for_status_events,
};

mod support;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_sequence_opprett_internt_notat_avslutt_sak() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    let commands = scenario.build_sequence(
        "Z12345",
        "42",
        format!("Skuffen E2E test {}", Uuid::new_v4()),
        format!("Internt notat {}", Uuid::new_v4()),
    );
    send_command_batch(&env.nats_url, &commands).await?;
    let events = wait_for_status_events(
        &env.nats_url,
        commands.iter().map(|c| c.command_id),
        Duration::from_secs(30),
    )
    .await?;
    assert_happy_path_stages(&events, commands.iter().map(|c| c.command_id));

    // OpprettSak terminal event should carry saksnummer
    let saksnummer = extract_saksnummer(&events, commands[0].command_id);
    assert!(
        saksnummer.is_some(),
        "OpprettSak terminal Ok event should carry saksnummer"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_sequence_inngaende_journalpost_flow() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    // Step 1: Create sak, wait for saksnummer
    let opprett_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: scenario.sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel(format!(
                "Inngaende test {}",
                Uuid::new_v4()
            )),
            arkivdel: Arkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
            ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())?,
            tilgang: None,
        }),
    };
    send_command_batch(&env.nats_url, std::slice::from_ref(&opprett_sak)).await?;
    let sak_events = wait_for_status_events(
        &env.nats_url,
        [opprett_sak.command_id],
        Duration::from_secs(20),
    )
    .await?;
    let saksnummer = extract_saksnummer(&sak_events, opprett_sak.command_id)
        .expect("OpprettSak should return saksnummer");

    // Step 2: Create inngående journalpost referencing sak by arkiv_id
    let commands = vec![scenario.opprett_inngaende(
        "Z99999",
        "42",
        DtoSakKey::ArkivId(DtoSaksnummer::new(&saksnummer)?),
        "Inngaaende",
    )];
    send_command_batch(&env.nats_url, &commands).await?;
    let events = wait_for_status_events(
        &env.nats_url,
        commands.iter().map(|c| c.command_id),
        Duration::from_secs(20),
    )
    .await?;
    assert_happy_path_stages(&events, commands.iter().map(|c| c.command_id));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_sequence_utgaaende_journalpost_flow() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    // Step 1: Create sak, wait for saksnummer
    let opprett_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: scenario.sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel(format!(
                "Utgaaende test {}",
                Uuid::new_v4()
            )),
            arkivdel: Arkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
            ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())?,
            tilgang: None,
        }),
    };
    send_command_batch(&env.nats_url, std::slice::from_ref(&opprett_sak)).await?;
    let sak_events = wait_for_status_events(
        &env.nats_url,
        [opprett_sak.command_id],
        Duration::from_secs(20),
    )
    .await?;
    let saksnummer = extract_saksnummer(&sak_events, opprett_sak.command_id)
        .expect("OpprettSak should return saksnummer");

    // Step 2: Create utgående journalpost referencing sak by arkiv_id
    let commands = vec![scenario.opprett_utgaaende(
        "Z99999",
        "42",
        DtoSakKey::ArkivId(DtoSaksnummer::new(&saksnummer)?),
        "Utgaaende",
    )];
    send_command_batch(&env.nats_url, &commands).await?;
    let events = wait_for_status_events(
        &env.nats_url,
        commands.iter().map(|c| c.command_id),
        Duration::from_secs(20),
    )
    .await?;
    assert_happy_path_stages(&events, commands.iter().map(|c| c.command_id));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn query_hent_sak_via_nats_uses_id_mapping() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();

    // Create a sak first so we get a saksnummer
    let opprett_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: scenario.sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel(format!(
                "Query test {}",
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
    let events = wait_for_status_events(
        &env.nats_url,
        [opprett_sak.command_id],
        Duration::from_secs(20),
    )
    .await?;
    let saksnummer = extract_saksnummer(&events, opprett_sak.command_id)
        .expect("OpprettSak should return saksnummer");

    // Query by arkiv_id (uses id_mapping lookup internally)
    let response = hent_sak_via_nats_by_arkiv_id(&env.nats_url, &saksnummer).await?;
    assert_eq!(response.get("status").and_then(|s| s.as_str()), Some("Ok"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn query_hent_journalpost_via_nats() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();
    let response =
        hent_journalpost_via_nats(&env.nats_url, scenario.journalpost_internt_client_reference)
            .await?;
    assert_eq!(response.get("status").and_then(|s| s.as_str()), Some("Ok"));
    Ok(())
}

#[tokio::test]
async fn query_hent_bruker_mt_enheter_returns_not_implemented() -> Result<()> {
    let env = support::start_runtime().await?;

    let response = hent_bruker_mt_enheter_via_nats(&env.nats_url).await?;

    assert_eq!(
        response.get("status").and_then(|s| s.as_str()),
        Some("Error")
    );
    assert_eq!(
        response
            .get("payload")
            .and_then(|payload| payload.get("message"))
            .and_then(|message| message.as_str()),
        Some("Not implemented")
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn avslutt_sak_uten_journalposter_er_tillatt() -> Result<()> {
    let env = support::start_runtime().await?;

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
    let events = wait_for_status_events(
        &env.nats_url,
        commands.iter().map(|c| c.command_id),
        Duration::from_secs(30),
    )
    .await?;
    assert_happy_path_stages(&events, commands.iter().map(|c| c.command_id));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn avslutt_sak_med_arkiv_id_fullfoerer_gjennom_hele_flyten() -> Result<()> {
    let env = support::start_runtime().await?;

    // Step 1: Create a sak to get a saksnummer
    let sak_client_reference = Uuid::new_v4();
    let opprett_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel(format!(
                "Avslutt med arkiv_id {}",
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
    let sak_events = wait_for_status_events(
        &env.nats_url,
        [opprett_sak.command_id],
        Duration::from_secs(20),
    )
    .await?;
    let saksnummer = extract_saksnummer(&sak_events, opprett_sak.command_id)
        .expect("OpprettSak should return saksnummer");

    // Step 2: Avslutt sak using the saksnummer (arkiv_id)
    let avslutt_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::AvsluttSak(AvsluttSak {
            sak_key: DtoSakKey::ArkivId(DtoSaksnummer::new(&saksnummer)?),
        }),
    };
    send_command_batch(&env.nats_url, std::slice::from_ref(&avslutt_sak)).await?;
    let events = wait_for_status_events(
        &env.nats_url,
        [avslutt_sak.command_id],
        Duration::from_secs(20),
    )
    .await?;
    assert_happy_path_stages(&events, [avslutt_sak.command_id]);

    // Verify saksnummer is present in the AvsluttSak terminal event
    let avslutt_saksnummer = extract_saksnummer(&events, avslutt_sak.command_id);
    assert_eq!(avslutt_saksnummer.as_deref(), Some(saksnummer.as_str()));

    Ok(())
}

fn assert_happy_path_stages(
    events: &[SkuffenStatusEventV1],
    command_ids: impl IntoIterator<Item = Uuid>,
) {
    for command_id in command_ids {
        let command_events: Vec<&SkuffenStatusEventV1> = events
            .iter()
            .filter(|event| event.command_id == command_id)
            .collect();
        assert!(
            command_events
                .iter()
                .any(|event| event.phase == SkuffenStatusPhase::Ingest),
            "Missing Ingest event for command {command_id}"
        );
        assert!(
            command_events
                .iter()
                .any(|event| event.phase == SkuffenStatusPhase::Validate
                    && event.status == SkuffenStatus::Ok),
            "Missing Validate+Ok event for command {command_id}"
        );
        assert!(
            command_events
                .iter()
                .any(|event| event.phase == SkuffenStatusPhase::Execution
                    && event.status == SkuffenStatus::Pending),
            "Missing Execution+Pending event for command {command_id}"
        );
        assert!(
            command_events
                .iter()
                .any(|event| event.phase == SkuffenStatusPhase::Execution
                    && event.status == SkuffenStatus::Ok
                    && event.terminal),
            "Missing terminal Execution+Ok event for command {command_id}"
        );
    }
}
