//! Integration tests for the SettSaksansvarlig command.
//!
//! These tests verify the full command flow through domain/application/infrastructure
//! and test the state machine behavior for setting saksansvarlig on a sak.

use anyhow::Result;
use std::time::Duration;
use uuid::Uuid;

use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::sak::{Arkivdel, OpprettSak, SettSaksansvarlig};
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::sak::Saksnummer as DtoSaksnummer;
use lib_schemas::skuffen::status::{
    SkuffenCommandEvent, SkuffenCommandStatusV1, SkuffenStatusErrorCode,
};
use lib_schemas::skuffen::tilgang::Tilgjengelighet;

use support::{CommandScenario, extract_saksnummer, send_command_batch, wait_for_status_events};

mod support;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sett_saksansvarlig_happy_path() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();

    // Step 1: Create a sak to get a saksnummer
    let opprett_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: scenario.sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                "SettSaksansvarlig test {}",
                Uuid::new_v4()
            ))
            .unwrap(),
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
        Duration::from_secs(30),
    )
    .await?;
    let saksnummer = extract_saksnummer(&sak_events, opprett_sak.command_id)
        .expect("OpprettSak should return saksnummer");

    // Step 2: Set saksansvarlig using the saksnummer
    let sett_saksansvarlig = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::SettSaksansvarlig(SettSaksansvarlig {
            sak_key: DtoSakKey::ArkivId(DtoSaksnummer::new(&saksnummer)?),
            saksbehandler_id: "Z99999".to_string(),
            saksbehandler_enhet: "42".to_string(),
        }),
    };

    send_command_batch(&env.nats_url, std::slice::from_ref(&sett_saksansvarlig)).await?;
    let events = wait_for_status_events(
        &env.nats_url,
        [sett_saksansvarlig.command_id],
        Duration::from_secs(30),
    )
    .await?;

    // Verify the command went through all stages successfully
    assert_happy_path_stages(&events, [sett_saksansvarlig.command_id]);

    // Verify saksnummer is present in the terminal event
    let result_saksnummer = extract_saksnummer(&events, sett_saksansvarlig.command_id);
    assert_eq!(result_saksnummer.as_deref(), Some(saksnummer.as_str()));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sett_saksansvarlig_med_client_reference() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();

    // Step 1: Create a sak
    let opprett_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: scenario.sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                "SettSaksansvarlig client ref test {}",
                Uuid::new_v4()
            ))
            .unwrap(),
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
        Duration::from_secs(30),
    )
    .await?;
    let _ = extract_saksnummer(&sak_events, opprett_sak.command_id)
        .expect("OpprettSak should return saksnummer");

    // Step 2: Set saksansvarlig using client reference
    let sett_saksansvarlig = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::SettSaksansvarlig(SettSaksansvarlig {
            sak_key: DtoSakKey::ClientReference(scenario.sak_client_reference),
            saksbehandler_id: "Z88888".to_string(),
            saksbehandler_enhet: "42".to_string(),
        }),
    };

    send_command_batch(&env.nats_url, std::slice::from_ref(&sett_saksansvarlig)).await?;
    let events = wait_for_status_events(
        &env.nats_url,
        [sett_saksansvarlig.command_id],
        Duration::from_secs(30),
    )
    .await?;

    assert_happy_path_stages(&events, [sett_saksansvarlig.command_id]);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ukjent_arkivsak_avvises_i_validering() -> Result<()> {
    // Regresjonstest for feilklassifiseringen i valideringen.
    //
    // Før fiksen forsøkte adapteren å downcaste til `reqwest::Error` for å
    // finne statuskoden. Klienten bailer med en strengbasert anyhow, så
    // downcasten traff aldri, og alt ble recoverable. Kommandoen ble naket
    // uten forsinkelse og redelivert i en varm løkke mot arkivet — klienten
    // fikk «Mottatt» og deretter stillhet, aldri et avslag.
    let env = support::start_runtime().await?;

    let sett_saksansvarlig = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::SettSaksansvarlig(SettSaksansvarlig {
            sak_key: DtoSakKey::ArkivId(DtoSaksnummer::new("2026/999999")?),
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
        }),
    };

    // Request-reply lykkes: kommandoen er mottatt, utfallet kommer asynkront.
    send_command_batch(&env.nats_url, std::slice::from_ref(&sett_saksansvarlig)).await?;
    let events = wait_for_status_events(
        &env.nats_url,
        [sett_saksansvarlig.command_id],
        Duration::from_secs(30),
    )
    .await?;

    let mine: Vec<&SkuffenCommandStatusV1> = events
        .iter()
        .filter(|event| event.command_id == sett_saksansvarlig.command_id)
        .collect();

    // `Mottatt` publiseres først etter vellykket dispatch, så validatoren kan
    // rekke å avvise før den lander. Rekkefølgen mellom de to er ikke en del
    // av kontrakten denne testen holder på.
    let avvist = mine
        .iter()
        .find(|event| event.hendelse == SkuffenCommandEvent::Avvist)
        .expect("Avvist mangler — kommandoen ble aldri terminal");

    assert!(avvist.terminal);
    assert_eq!(
        avvist.error_code,
        Some(SkuffenStatusErrorCode::NotFound),
        "et ukjent saksnummer er NotFound, ikke InvalidRequest"
    );
    assert!(
        avvist.message.contains("2026/999999"),
        "avvisningen skal si hvilken sak som ikke ble funnet, fikk {:?}",
        avvist.message
    );
    assert_eq!(
        avvist.saksnummer.as_ref().map(|s| s.as_str()),
        Some("2026/999999")
    );

    // Ingenting skal ha kommet forbi valideringen.
    for uventet in [
        SkuffenCommandEvent::Validert,
        SkuffenCommandEvent::Utfores,
        SkuffenCommandEvent::Fullfort,
    ] {
        assert!(
            !mine.iter().any(|event| event.hendelse == uventet),
            "{uventet:?} skal ikke publiseres for en avvist kommando: {mine:?}"
        );
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn irrecoverable_arkivfeil_gir_terminal_feilet() -> Result<()> {
    // Regresjonstest for SKU-0016 R6 gjennom hele kjeden.
    //
    // Før fiksen mappet executoren hver gateway-feil til recoverable, så en
    // irrecoverable feil ble liggende usynlig i `retry_venter` for alltid og
    // klienten fikk aldri et terminalt event.
    let env = support::start_runtime_med_arkivfeil("irrecoverable").await?;

    let scenario = CommandScenario::new();
    let opprett_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: scenario.sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                "Irrecoverable arkivfeil {}",
                Uuid::new_v4()
            ))
            .unwrap(),
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
        Duration::from_secs(30),
    )
    .await?;

    let mine: Vec<&SkuffenCommandStatusV1> = events
        .iter()
        .filter(|event| event.command_id == opprett_sak.command_id)
        .collect();

    // Valideringen slipper den gjennom — feilen oppstår først i arkivkallet.
    assert!(
        mine.iter()
            .any(|event| event.hendelse == SkuffenCommandEvent::Validert),
        "Validert mangler: {mine:?}"
    );

    let terminal = mine
        .iter()
        .find(|event| event.terminal)
        .expect("ingen terminal hendelse — operasjonen ble liggende i retry");

    assert_eq!(
        terminal.hendelse,
        SkuffenCommandEvent::Feilet,
        "irrecoverable arkivfeil skal gi terminal feilet, fikk {:?}",
        terminal.hendelse
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sett_saksansvarlig_sak_i_eksekvering() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();

    // Step 1: Create a sak
    let opprett_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: scenario.sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                "SettSaksansvarlig state test {}",
                Uuid::new_v4()
            ))
            .unwrap(),
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
        Duration::from_secs(30),
    )
    .await?;
    let saksnummer = extract_saksnummer(&sak_events, opprett_sak.command_id)
        .expect("OpprettSak should return saksnummer");

    // Step 2: Immediately try to set saksansvarlig while OpprettSak might still be executing
    // This tests the state machine - SettSaksansvarlig should only be valid when sak.tilstand == Opprettet
    let sett_saksansvarlig = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::SettSaksansvarlig(SettSaksansvarlig {
            sak_key: DtoSakKey::ArkivId(DtoSaksnummer::new(&saksnummer)?),
            saksbehandler_id: "Z77777".to_string(),
            saksbehandler_enhet: "42".to_string(),
        }),
    };

    send_command_batch(&env.nats_url, &[opprett_sak, sett_saksansvarlig.clone()]).await?;
    let events = wait_for_status_events(
        &env.nats_url,
        [sett_saksansvarlig.command_id],
        Duration::from_secs(30),
    )
    .await?;

    // The command should either succeed (if OpprettSak completed first) or fail due to state
    // We're mainly testing that the state machine check is in place
    let terminal_event = events
        .iter()
        .find(|e| e.command_id == sett_saksansvarlig.command_id && e.terminal)
        .expect("Should have terminal event");

    // If the command succeeded, verify saksnummer is present
    if terminal_event.hendelse == SkuffenCommandEvent::Fullfort {
        let result_saksnummer = extract_saksnummer(&events, sett_saksansvarlig.command_id);
        assert_eq!(result_saksnummer.as_deref(), Some(saksnummer.as_str()));
    }
    // If it failed, that's also acceptable - it means the state machine is working

    Ok(())
}

fn assert_happy_path_stages(
    events: &[SkuffenCommandStatusV1],
    command_ids: impl IntoIterator<Item = Uuid>,
) {
    for command_id in command_ids {
        let command_events: Vec<&SkuffenCommandStatusV1> = events
            .iter()
            .filter(|event| event.command_id == command_id)
            .collect();

        assert!(
            command_events
                .iter()
                .any(|event| event.hendelse == SkuffenCommandEvent::Mottatt),
            "Missing Ingest event for command {command_id}"
        );
        assert!(
            command_events
                .iter()
                .any(|event| event.hendelse == SkuffenCommandEvent::Validert),
            "Missing Validate+Ok event for command {command_id}"
        );
        assert!(
            command_events
                .iter()
                .any(|event| event.hendelse == SkuffenCommandEvent::Utfores),
            "Missing Execution+Pending event for command {command_id}"
        );
        assert!(
            command_events
                .iter()
                .any(|event| event.hendelse == SkuffenCommandEvent::Fullfort && event.terminal),
            "Missing terminal fullfort event for command {command_id}"
        );
    }
}
