//! Admin read ende-til-ende over NATS core request-reply.
//!
//! Testene låser den klientvendte kontrakten: to eksakte subjects, stabile
//! feilmeldinger, queue group som gir nøyaktig ett svar, og en size-guard som
//! gir en forståelig feil i stedet for caller-timeout.

use std::time::Duration;

use anyhow::Result;
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use serde_json::{Value, json};
use uuid::Uuid;

use support::{
    ADMIN_COMMAND_SUBJECT, ADMIN_SAK_SUBJECT, CommandScenario, admin_hent_command, admin_hent_sak,
    admin_raw_request, admin_raw_request_alle_svar, publish_media, send_command_batch,
    wait_for_queue_members, wait_for_status_events,
};

mod support;

const COMMAND_QUEUE_GROUP: &str = "skuffen-admin-read-command-hent-v1";
const SAK_QUEUE_GROUP: &str = "skuffen-admin-read-sak-hent-v1";

fn feilmelding(svar: &Value) -> &str {
    svar["payload"]["message"].as_str().unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ukjente_ressurser_gir_stabile_feilsvar_paa_begge_subjects() -> Result<()> {
    let env = support::start_runtime().await?;

    let command = admin_hent_command(&env.nats_url, Uuid::new_v4()).await?;
    assert_eq!(command["status"], "Error");
    assert_eq!(feilmelding(&command), "Command not found");

    for key in [
        json!({ "type": "skuffenId", "value": Uuid::new_v4() }),
        json!({ "type": "clientReference", "value": Uuid::new_v4() }),
        json!({ "type": "arkivId", "value": "2026/ukjent" }),
    ] {
        let sak = admin_hent_sak(&env.nats_url, key).await?;
        assert_eq!(sak["status"], "Error");
        assert_eq!(feilmelding(&sak), "Sak not found");
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ugyldige_requester_gir_invalid_request_format() -> Result<()> {
    let env = support::start_runtime().await?;

    let ugyldige = vec![
        (ADMIN_COMMAND_SUBJECT, "{ikke json".to_string()),
        (
            ADMIN_COMMAND_SUBJECT,
            json!({ "command_id": Uuid::new_v4() }).to_string(),
        ),
        (
            ADMIN_COMMAND_SUBJECT,
            json!({ "utfort_av": "   ", "command_id": Uuid::new_v4() }).to_string(),
        ),
        (
            ADMIN_COMMAND_SUBJECT,
            json!({
                "utfort_av": "test-operator",
                "command_id": Uuid::new_v4(),
                "ukjent_felt": true
            })
            .to_string(),
        ),
        (
            ADMIN_SAK_SUBJECT,
            json!({
                "utfort_av": "",
                "key": { "type": "skuffenId", "value": Uuid::new_v4() }
            })
            .to_string(),
        ),
    ];

    for (subject, payload) in ugyldige {
        let svar = admin_raw_request(&env.nats_url, subject, &payload).await?;
        assert_eq!(svar["status"], "Error", "subject {subject}: {svar}");
        assert_eq!(feilmelding(&svar), "Invalid request format");
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn command_og_sak_kan_hentes_etter_command_flow() -> Result<()> {
    let env = support::start_runtime().await?;
    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;

    let commands = scenario.build_sequence(
        "Z12345",
        "42",
        "Adminsak".to_string(),
        "Internt notat".to_string(),
    );
    let opprett_sak_command_id = commands[0].command_id;
    let command_ids = send_command_batch(&env.nats_url, &commands).await?;
    wait_for_status_events(&env.nats_url, command_ids, Duration::from_secs(60)).await?;

    let command = admin_hent_command(&env.nats_url, opprett_sak_command_id).await?;
    assert_eq!(command["status"], "Ok", "{command}");
    let payload = &command["payload"];
    assert_eq!(payload["command_type"], "opprett_sak");
    assert_eq!(payload["utfall"], "fullfort");
    let operasjoner = payload["operasjoner"].as_array().expect("operasjoner");
    assert!(!operasjoner.is_empty());
    assert_eq!(operasjoner[0]["operasjonstype"], "opprett_sak");
    assert_eq!(operasjoner[0]["entitet"]["entitet_type"], "sak");

    let per_client_reference = admin_hent_sak(
        &env.nats_url,
        json!({ "type": "clientReference", "value": scenario.sak_client_reference }),
    )
    .await?;
    assert_eq!(
        per_client_reference["status"], "Ok",
        "{per_client_reference}"
    );
    let sak = &per_client_reference["payload"];
    assert_eq!(
        sak["identitet"]["client_reference"],
        json!(scenario.sak_client_reference)
    );
    assert_eq!(sak["fakta"]["sakstittel"], "Adminsak");
    // Saksbehandlerkontekstene er separate begreper og flates ikke ut.
    assert_eq!(sak["fakta"]["opprettelse_saksbehandler_id"], "Z12345");
    let journalposter = sak["fakta"]["journalposter"]
        .as_array()
        .expect("journalposter");
    assert_eq!(journalposter.len(), 1);
    assert_eq!(journalposter[0]["saksbehandler_id"], "Z12345");
    assert!(
        !journalposter[0]["dokumenter"]
            .as_array()
            .expect("dokumenter")
            .is_empty()
    );
    assert!(
        !sak["operasjoner"]
            .as_array()
            .expect("operasjoner")
            .is_empty()
    );

    // Skuffen-id hentes fra admin-responsen selv, ikke fra en egen fixture.
    let skuffen_id = sak["identitet"]["skuffen_id"].clone();
    let per_skuffen_id = admin_hent_sak(
        &env.nats_url,
        json!({ "type": "skuffenId", "value": skuffen_id }),
    )
    .await?;
    assert_eq!(per_skuffen_id, per_client_reference);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn to_admin_listenere_i_samme_queue_group_gir_noyaktig_ett_svar() -> Result<()> {
    let env = support::start_runtime().await?;

    // Andre listener mot samme NATS og database, i de samme queue groupene.
    let shutdown = tokio_util::sync::CancellationToken::new();
    let andre_listener = infrastructure::bootstrap::build_admin_listener(
        infrastructure::nats::setup::setup_nats().await?,
        infrastructure::database::setup::setup_database().await?,
        shutdown.clone(),
    );
    let handle = tokio::spawn(async move { andre_listener.run().await });

    for (subject, queue_group) in [
        (ADMIN_COMMAND_SUBJECT, COMMAND_QUEUE_GROUP),
        (ADMIN_SAK_SUBJECT, SAK_QUEUE_GROUP),
    ] {
        wait_for_queue_members(
            &env.nats_monitor_url,
            subject,
            queue_group,
            2,
            Duration::from_secs(15),
        )
        .await?;
    }

    let svar = admin_raw_request_alle_svar(
        &env.nats_url,
        ADMIN_COMMAND_SUBJECT,
        &json!({ "utfort_av": "test-operator", "command_id": Uuid::new_v4() }).to_string(),
        Duration::from_millis(750),
    )
    .await?;

    assert_eq!(
        svar.len(),
        1,
        "queue group skal gi nøyaktig ett svar, fikk {svar:?}"
    );

    shutdown.cancel();
    handle
        .await?
        .expect("nedstenging av admin-listener er ikke en feil");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn for_stort_svar_gir_response_too_large_ikke_timeout() -> Result<()> {
    // Grensen er satt lavt nok til at en helt vanlig sak vokser forbi den.
    // Scenarioet bruker ingen mediareferanser, så media-protokollens chunk
    // size er irrelevant her.
    let env = support::start_runtime_med_max_payload(4096).await?;
    let scenario = CommandScenario::new();

    let opprett = scenario.opprett_sak_med_tilgjengelighet(
        "Z12345",
        "42",
        "Adminsak".to_string(),
        lib_schemas::skuffen::tilgang::Tilgjengelighet::Offentlig,
    );
    let command_ids = send_command_batch(&env.nats_url, &[opprett]).await?;
    wait_for_status_events(&env.nats_url, command_ids, Duration::from_secs(60)).await?;

    let key = json!({ "type": "clientReference", "value": scenario.sak_client_reference });
    let mut siste = admin_hent_sak(&env.nats_url, key.clone()).await?;
    assert_eq!(siste["status"], "Ok", "{siste}");

    for runde in 0..10 {
        let batch: Vec<_> = (0..5)
            .map(|nr| {
                scenario.sett_saksansvarlig(
                    DtoSakKey::ClientReference(scenario.sak_client_reference),
                    &format!("Z{runde}{nr}0000"),
                    "42",
                )
            })
            .collect();
        let command_ids = send_command_batch(&env.nats_url, &batch).await?;
        wait_for_status_events(&env.nats_url, command_ids, Duration::from_secs(60)).await?;

        siste = admin_hent_sak(&env.nats_url, key.clone()).await?;
        if siste["status"] == "Error" {
            assert_eq!(feilmelding(&siste), "Response too large");
            return Ok(());
        }
    }

    panic!("sak-responsen vokste aldri forbi grensen: {siste}");
}
