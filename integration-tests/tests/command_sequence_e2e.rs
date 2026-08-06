use anyhow::Result;
use std::time::Duration;
use uuid::Uuid;

use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::sak::Saksnummer as DtoSaksnummer;
use lib_schemas::skuffen::status::{SkuffenStatus, SkuffenStatusEventV1, SkuffenStatusPhase};
use lib_schemas::skuffen::tilgang::Tilgjengelighet;

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
            tilgjengelighet: Tilgjengelighet::Offentlig,
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
            tilgjengelighet: Tilgjengelighet::Offentlig,
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
            tilgjengelighet: Tilgjengelighet::Offentlig,
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
                tilgjengelighet: Tilgjengelighet::Offentlig,
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
            tilgjengelighet: Tilgjengelighet::Offentlig,
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn skjermet_sak_med_skjermet_internt_notat_happy_path() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    let opprett_sak = scenario.opprett_sak_med_tilgjengelighet(
        "Z12345",
        "42",
        format!("Skjermet sak {}", Uuid::new_v4()),
        CommandScenario::skjermet(),
    );
    let internt_notat = scenario.opprett_skjermet_internt_notat(
        "Z12345",
        "42",
        DtoSakKey::ClientReference(scenario.sak_client_reference),
        "[|Ola Norrmann|] - Skjermet",
    );

    let commands = vec![opprett_sak, internt_notat];
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
async fn internt_notat_med_ugyldig_markup_avvises_i_validering() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    let opprett_sak = scenario.opprett_sak_med_tilgjengelighet(
        "Z12345",
        "42",
        format!("Markup avvisning {}", Uuid::new_v4()),
        Tilgjengelighet::Offentlig,
    );
    send_command_batch(&env.nats_url, std::slice::from_ref(&opprett_sak)).await?;
    let sak_events = wait_for_status_events(
        &env.nats_url,
        [opprett_sak.command_id],
        Duration::from_secs(20),
    )
    .await?;
    extract_saksnummer(&sak_events, opprett_sak.command_id)
        .expect("OpprettSak should return saksnummer");

    let internt_notat = scenario.opprett_internt_notat_med_ugyldig_markup(
        "Z12345",
        "42",
        DtoSakKey::ClientReference(scenario.sak_client_reference),
        "Notat med [skjermet] uten skjerming",
    );
    send_command_batch(&env.nats_url, std::slice::from_ref(&internt_notat)).await?;
    let events = wait_for_status_events(
        &env.nats_url,
        [internt_notat.command_id],
        Duration::from_secs(20),
    )
    .await?;
    assert_validate_error(&events, internt_notat.command_id);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn utgaaende_med_utsending_flow() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    let opprett_sak = scenario.opprett_sak_med_tilgjengelighet(
        "Z99999",
        "42",
        format!("Utgaaende utsending {}", Uuid::new_v4()),
        Tilgjengelighet::Offentlig,
    );
    send_command_batch(&env.nats_url, std::slice::from_ref(&opprett_sak)).await?;
    let sak_events = wait_for_status_events(
        &env.nats_url,
        [opprett_sak.command_id],
        Duration::from_secs(20),
    )
    .await?;
    let saksnummer = extract_saksnummer(&sak_events, opprett_sak.command_id)
        .expect("OpprettSak should return saksnummer");

    let commands = vec![scenario.opprett_utgaaende_med_utsending(
        "Z99999",
        "42",
        DtoSakKey::ArkivId(DtoSaksnummer::new(&saksnummer)?),
        "Utgaaende med utsending",
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
async fn utgaaende_med_flere_mottakere_flow() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    let opprett_sak = scenario.opprett_sak_med_tilgjengelighet(
        "Z99999",
        "42",
        format!("Utgaaende flere mottakere {}", Uuid::new_v4()),
        Tilgjengelighet::Offentlig,
    );
    send_command_batch(&env.nats_url, std::slice::from_ref(&opprett_sak)).await?;
    let sak_events = wait_for_status_events(
        &env.nats_url,
        [opprett_sak.command_id],
        Duration::from_secs(20),
    )
    .await?;
    let saksnummer = extract_saksnummer(&sak_events, opprett_sak.command_id)
        .expect("OpprettSak should return saksnummer");

    let commands = vec![scenario.opprett_utgaaende_flere_mottakere(
        "Z99999",
        "42",
        DtoSakKey::ArkivId(DtoSaksnummer::new(&saksnummer)?),
        "Utgaaende flere mottakere",
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

fn assert_validate_error(events: &[SkuffenStatusEventV1], command_id: Uuid) {
    let command_events: Vec<&SkuffenStatusEventV1> = events
        .iter()
        .filter(|event| event.command_id == command_id)
        .collect();

    assert!(
        command_events
            .iter()
            .any(|event| event.phase == SkuffenStatusPhase::Validate
                && event.status == SkuffenStatus::Error),
        "Expected Validate+Error event for command {command_id}, got {command_events:?}"
    );
    assert!(
        !command_events
            .iter()
            .any(|event| event.phase == SkuffenStatusPhase::Execution
                && event.status == SkuffenStatus::Ok),
        "Rejected command {command_id} must not reach Execution+Ok"
    );
}
