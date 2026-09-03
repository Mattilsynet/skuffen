use anyhow::Result;
use async_nats::jetstream;
use bytes::Bytes;
use lib_nats::chunked_upload::{
    ChunkedUploadClient, ChunkedUploadClientConfig, UploadError, UploadErrorCode, UploadRequest,
};
use std::time::Duration;
use uuid::Uuid;

use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::sak::Saksnummer as DtoSaksnummer;
use lib_schemas::skuffen::status::{SkuffenCommandEvent, SkuffenCommandStatusV1};
use lib_schemas::skuffen::tilgang::Tilgjengelighet;

use support::{
    CommandScenario, extract_saksnummer, hent_bruker_mt_enheter_via_nats,
    hent_journalpost_via_nats, hent_sak_via_nats_by_arkiv_id, publish_media, send_command_batch,
    send_raw_command_payload, wait_for_status_events,
};

mod support;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn media_upload_is_chunked_idempotent_and_immutable() -> Result<()> {
    let env = support::start_runtime().await?;
    let dokument_referanse = Uuid::new_v4();

    publish_media(&env.nats_url, dokument_referanse).await?;
    publish_media(&env.nats_url, dokument_referanse).await?;

    let client = async_nats::connect(&env.nats_url).await?;
    let uploader = ChunkedUploadClient::new(
        client,
        ChunkedUploadClientConfig {
            base_subject: "arkiv.arkiver.media".to_string(),
            ..ChunkedUploadClientConfig::default()
        },
    );
    let error = uploader
        .upload(UploadRequest {
            upload_id: dokument_referanse.to_string(),
            bytes: Bytes::from_static(b"different content"),
            filename: Some("vedlegg.txt".to_string()),
            content_type: Some("text/plain".to_string()),
        })
        .await
        .expect_err("same upload ID with different content must conflict");

    assert!(matches!(
        error,
        UploadError::Rejected {
            code: UploadErrorCode::UploadConflict,
            ..
        }
    ));
    let store = jetstream::new(async_nats::connect(&env.nats_url).await?)
        .get_object_store("arkiv_media")
        .await?;
    let mut object = store.get(dokument_referanse.to_string()).await?;
    let mut stored = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut object, &mut stored).await?;
    let expected: Vec<u8> = (0..2_000_001).map(|index| (index % 251) as u8).collect();
    assert_eq!(stored, expected);
    Ok(())
}

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
            sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                "Inngaende test {}",
                Uuid::new_v4()
            ))
            .unwrap(),
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
            sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                "Utgaaende test {}",
                Uuid::new_v4()
            ))
            .unwrap(),
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
async fn query_hent_sak_via_nats_slaar_opp_entitet() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();

    // Create a sak first so we get a saksnummer
    let opprett_sak = CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: scenario.sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                "Query test {}",
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
        Duration::from_secs(20),
    )
    .await?;
    let saksnummer = extract_saksnummer(&events, opprett_sak.command_id)
        .expect("OpprettSak should return saksnummer");

    // Query by arkiv_id (slår opp entitet internt)
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
                sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                    "Skuffen E2E avslutt uten journalposter {}",
                    Uuid::new_v4()
                ))
                .unwrap(),
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
            sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                "Avslutt med arkiv_id {}",
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

fn assert_validate_error(events: &[SkuffenCommandStatusV1], command_id: Uuid) {
    let command_events: Vec<&SkuffenCommandStatusV1> = events
        .iter()
        .filter(|event| event.command_id == command_id)
        .collect();

    assert!(
        command_events
            .iter()
            .any(|event| event.hendelse == SkuffenCommandEvent::Avvist),
        "Expected avvist event for command {command_id}, got {command_events:?}"
    );
    assert!(
        !command_events
            .iter()
            .any(|event| event.hendelse == SkuffenCommandEvent::Fullfort),
        "Rejected command {command_id} must never reach fullfort"
    );
}

/// Ugyldige payloads skal avvises på wire-grensen (deserialisering) med en
/// `Error`-kvittering, og aldri komme inn i pipelinen. Dekker regresjoner der
/// en `#[serde(try_from)]`-validering eller `deny_unknown_fields` fjernes.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ugyldig_payload_avvises_paa_wire_grensen() -> Result<()> {
    let env = support::start_runtime().await?;

    let common = |sakstittel: &str, ekstra: &str| {
        format!(
            r#"[{{"command_id":"{cid}","correlation_id":null,"payload":{{"OpprettSak":{{"client_reference":"{cref}","sakstittel":"{sakstittel}","arkivdel":"Tilsynsdivisjonene","saksbehandler_id":"Z12345","saksbehandler_enhet":"42","ordningsverdi":"123","tilgjengelighet":"Offentlig"{ekstra}}}}}}}]"#,
            cid = Uuid::new_v4(),
            cref = Uuid::new_v4(),
        )
    };

    // Hver ugyldig payload skal gi en Error-kvittering.
    let for_lang_tittel = common(&"A".repeat(300), "");
    let tom_tittel = common("", "");
    let ukjent_felt = common("Gyldig", r#","evil_injected_field":"x""#);

    let ugyldig_fnr = format!(
        r#"[{{"command_id":"{cid}","correlation_id":null,"payload":{{"OpprettUtgåendeJournalpostMedUtsending":{{"client_reference":"{cref}","tittel":"Test","dokument_dato":"2025-01-01","saksbehandler":"Z12345","saksbehandler_enhet":"42","tilgjengelighet":"Offentlig","dokumenter":[],"sak_key":{{"type":"clientReference","value":"{sref}"}},"kildesystem":null,"mottakere":[{{"navn":"Ola","id":{{"Person":{{"fødselsnummer":"12345678901"}}}},"adresse":{{"adresse":"Gata 1","postnummer":"0350","poststed":"Oslo"}}}}]}}}}}}]"#,
        cid = Uuid::new_v4(),
        cref = Uuid::new_v4(),
        sref = Uuid::new_v4(),
    );

    let skjermet_tom_kode = format!(
        r#"[{{"command_id":"{cid}","correlation_id":null,"payload":{{"OpprettSak":{{"client_reference":"{cref}","sakstittel":"Test","arkivdel":"Tilsynsdivisjonene","saksbehandler_id":"Z12345","saksbehandler_enhet":"42","ordningsverdi":"123","tilgjengelighet":{{"Skjermet":{{"tilgangskode":"","tilgangshjemmel":"Offl. § 13"}}}}}}}}}}]"#,
        cid = Uuid::new_v4(),
        cref = Uuid::new_v4(),
    );

    for (beskrivelse, payload) in [
        ("for lang sakstittel", &for_lang_tittel),
        ("tom sakstittel", &tom_tittel),
        ("ukjent felt", &ukjent_felt),
        ("ugyldig fødselsnummer", &ugyldig_fnr),
        ("skjermet med tom tilgangskode", &skjermet_tom_kode),
    ] {
        let kvittering = send_raw_command_payload(&env.nats_url, payload).await?;
        assert!(
            kvittering.get("Error").is_some(),
            "'{beskrivelse}' skulle gitt Error-kvittering, fikk: {kvittering}"
        );
    }

    // Positiv kontroll: en gyldig payload skal gi Ok-kvittering.
    let gyldig = common("Gyldig kontrolltittel", "");
    let kvittering = send_raw_command_payload(&env.nats_url, &gyldig).await?;
    assert!(
        kvittering.get("Ok").is_some(),
        "Gyldig payload skulle gitt Ok-kvittering, fikk: {kvittering}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Ett beslutningssted (SKU-0020 R1)
// ---------------------------------------------------------------------------
//
// Regresjonstestene for hovedbugen. Før SKU-0020 traff evalueringspasset
// operasjonen først, skrev den terminal og publiserte ingenting. For
// kommandoer som dekomponerer til én operasjon ble hele kommandoen stille for
// alltid — klienten fikk `Utfores` og deretter ingenting.

fn opprett_sak_for(client_reference: Uuid, tittel: &str) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                "{tittel} {}",
                Uuid::new_v4()
            ))
            .unwrap(),
            arkivdel: Arkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())
                .unwrap(),
            tilgjengelighet: Tilgjengelighet::Offentlig,
        }),
    }
}

fn avslutt_sak_for(client_reference: Uuid) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::AvsluttSak(AvsluttSak {
            sak_key: DtoSakKey::ClientReference(client_reference),
        }),
    }
}

fn assert_terminal_hendelse(
    events: &[SkuffenCommandStatusV1],
    command_id: Uuid,
    forventet: SkuffenCommandEvent,
) {
    let mine: Vec<&SkuffenCommandStatusV1> = events
        .iter()
        .filter(|event| event.command_id == command_id)
        .collect();

    let terminal = mine
        .iter()
        .find(|event| event.terminal)
        .unwrap_or_else(|| panic!("ingen terminal hendelse for {command_id}, fikk {mine:?}"));

    assert_eq!(
        terminal.hendelse, forventet,
        "feil terminal hendelse for {command_id}, fikk {mine:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn avslutt_sak_to_ganger_gir_terminalt_utfall_begge_ganger() -> Result<()> {
    let env = support::start_runtime().await?;

    let sak_client_reference = Uuid::new_v4();
    let opprett_sak = opprett_sak_for(sak_client_reference, "Avslutt to ganger");
    let forste = avslutt_sak_for(sak_client_reference);

    send_command_batch(&env.nats_url, &[opprett_sak.clone(), forste.clone()]).await?;
    let events = wait_for_status_events(
        &env.nats_url,
        [opprett_sak.command_id, forste.command_id],
        Duration::from_secs(30),
    )
    .await?;
    assert_terminal_hendelse(&events, forste.command_id, SkuffenCommandEvent::Fullfort);

    // Andre gang er saken allerede avsluttet. `AlleredeUtfort` er et
    // ok-utfall, og skal nå klienten som et terminalt event.
    let andre = avslutt_sak_for(sak_client_reference);
    send_command_batch(&env.nats_url, std::slice::from_ref(&andre)).await?;
    let events =
        wait_for_status_events(&env.nats_url, [andre.command_id], Duration::from_secs(30)).await?;

    assert_terminal_hendelse(&events, andre.command_id, SkuffenCommandEvent::Fullfort);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sett_saksansvarlig_uten_endring_gir_terminalt_utfall() -> Result<()> {
    let env = support::start_runtime().await?;

    let sak_client_reference = Uuid::new_v4();
    let opprett_sak = opprett_sak_for(sak_client_reference, "Saksansvarlig uten endring");
    let sak_key = || DtoSakKey::ClientReference(sak_client_reference);

    let scenario = CommandScenario::new();
    let forste = scenario.sett_saksansvarlig(sak_key(), "Z99999", "42");

    send_command_batch(&env.nats_url, &[opprett_sak.clone(), forste.clone()]).await?;
    let events = wait_for_status_events(
        &env.nats_url,
        [opprett_sak.command_id, forste.command_id],
        Duration::from_secs(30),
    )
    .await?;
    assert_terminal_hendelse(&events, forste.command_id, SkuffenCommandEvent::Fullfort);

    // Samme verdi en gang til: ingenting å gjøre i arkivet, men klienten skal
    // likevel få vite at forespørselen er ferdig.
    let andre = scenario.sett_saksansvarlig(sak_key(), "Z99999", "42");
    send_command_batch(&env.nats_url, std::slice::from_ref(&andre)).await?;
    let events =
        wait_for_status_events(&env.nats_url, [andre.command_id], Duration::from_secs(30)).await?;

    assert_terminal_hendelse(&events, andre.command_id, SkuffenCommandEvent::Fullfort);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn html_mal_som_vedlegg_gir_terminal_feilet_ikke_stillhet() -> Result<()> {
    let env = support::start_runtime().await?;

    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;
    publish_media(&env.nats_url, scenario.mal_referanse).await?;

    let opprett_sak = scenario.opprett_sak_med_tilgjengelighet(
        "Z12345",
        "42",
        format!("HTML-mal som vedlegg {}", Uuid::new_v4()),
        Tilgjengelighet::Offentlig,
    );
    send_command_batch(&env.nats_url, std::slice::from_ref(&opprett_sak)).await?;
    wait_for_status_events(
        &env.nats_url,
        [opprett_sak.command_id],
        Duration::from_secs(30),
    )
    .await?;

    // `LeggTilVedlegg` er `Ugyldig` ved første vurdering. Beslutningen tas i
    // executoren, som også publiserer — den kan ikke skrives terminal noe
    // annet sted.
    let notat = scenario.opprett_internt_notat_med_html_vedlegg(
        "Z12345",
        "42",
        DtoSakKey::ClientReference(scenario.sak_client_reference),
        "Notat med mal som vedlegg",
    );
    send_command_batch(&env.nats_url, std::slice::from_ref(&notat)).await?;
    let events =
        wait_for_status_events(&env.nats_url, [notat.command_id], Duration::from_secs(30)).await?;

    assert_terminal_hendelse(&events, notat.command_id, SkuffenCommandEvent::Feilet);
    Ok(())
}
