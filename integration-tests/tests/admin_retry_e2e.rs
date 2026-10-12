//! Admin retry ende-til-ende: eksplisitte kommandoer eller en hel sak slippes
//! inn i ordinær revalidering etter at forholdet i arkivet er rettet.

use std::time::Duration;

use anyhow::Result;
use application::command::ports::entitet_port::{EntitetRepository, NyEntitet};
use async_nats::jetstream::{self, consumer::PullConsumer};
use domain::eksekvering::operasjon::EntitetType;
use futures::StreamExt;
use infrastructure::command::adapter::fake_command_state_repo::registrer_fake_sak;
use infrastructure::command::adapter::postgres_entitet_repository::PostgresEntitetRepository;
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::sak::Saksnummer as DtoSaksnummer;
use lib_schemas::skuffen::status::{
    SkuffenCommandEvent, SkuffenCommandStatusV1, SkuffenStatusErrorCode,
};
use serde_json::{Value, json};
use uuid::Uuid;

use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::sak::AvsluttSak;

use support::{
    ADMIN_RETRY_COMMANDS_SUBJECT, ADMIN_RETRY_SAK_SUBJECT, CommandScenario, admin_hent_command,
    admin_raw_request, admin_retry, publish_media, send_command_batch,
    wait_for_admin_retry_responders, wait_for_command_event,
};

mod support;

const VENT: Duration = Duration::from_secs(30);

fn assert_ack(svar: &Value) {
    assert_eq!(*svar, json!({ "status": "Ok", "payload": null }));
}

async fn nye_command_events(nats_url: &str, command_id: Uuid) -> Result<PullConsumer> {
    let client = async_nats::connect(nats_url).await?;
    let stream = jetstream::new(client).get_stream("arkiv_status").await?;
    Ok(stream
        .create_consumer(jetstream::consumer::pull::Config {
            ack_policy: jetstream::consumer::AckPolicy::None,
            deliver_policy: jetstream::consumer::DeliverPolicy::New,
            filter_subject: format!("arkiv.status.{command_id}.command"),
            ..Default::default()
        })
        .await?)
}

async fn vent_pa_ny_avvisning(consumer: &PullConsumer, command_id: Uuid) -> Result<()> {
    let mut messages = consumer.messages().await?;
    tokio::time::timeout(VENT, async {
        while let Some(message) = messages.next().await {
            let event: SkuffenCommandStatusV1 = serde_json::from_slice(&message?.payload)?;
            assert_eq!(event.command_id, command_id);
            if event.hendelse == SkuffenCommandEvent::Avvist {
                assert_eq!(event.error_code, Some(SkuffenStatusErrorCode::NotFound));
                assert!(event.terminal);
                return Ok(());
            }
            assert_ne!(event.hendelse, SkuffenCommandEvent::Fullfort, "{event:?}");
        }
        anyhow::bail!("Statusstrømmen ble lukket før ny avvisning for {command_id}")
    })
    .await
    .map_err(|_| anyhow::anyhow!("Ventet for lenge på ny avvisning for {command_id}"))?
}

async fn hent_fullfort_command(nats_url: &str, command_id: Uuid) -> Result<Value> {
    let snapshot = admin_hent_command(nats_url, command_id).await?;
    assert_eq!(snapshot["status"], "Ok", "{snapshot}");
    assert_eq!(snapshot["payload"]["utfall"], "fullfort", "{snapshot}");
    assert!(
        !snapshot["payload"]["operasjoner"]
            .as_array()
            .expect("operasjoner")
            .is_empty(),
        "{snapshot}"
    );
    Ok(snapshot)
}

fn unikt_saksnummer() -> String {
    format!("2026/{}", 900_000 + (Uuid::new_v4().as_u128() % 99_999))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn avvist_kommando_fullfores_etter_retting_i_arkivet() -> Result<()> {
    let env = support::start_runtime().await?;
    wait_for_admin_retry_responders(&env.nats_url).await?;
    let saksnummer = unikt_saksnummer();
    let command = CommandScenario::new().sett_saksansvarlig(
        DtoSakKey::ArkivId(DtoSaksnummer::new(&saksnummer)?),
        "Z99999",
        "42",
    );

    send_command_batch(&env.nats_url, std::slice::from_ref(&command)).await?;
    let events = wait_for_command_event(
        &env.nats_url,
        command.command_id,
        SkuffenCommandEvent::Avvist,
        VENT,
    )
    .await?;
    assert_eq!(
        events.last().and_then(|event| event.error_code),
        Some(SkuffenStatusErrorCode::NotFound)
    );

    // Uten retting gir et nytt forsøk samme avvisning.
    let nye_events = nye_command_events(&env.nats_url, command.command_id).await?;
    let svar = admin_retry(&env.nats_url, &[command.command_id]).await?;
    assert_ack(&svar);
    vent_pa_ny_avvisning(&nye_events, command.command_id).await?;

    // Saken finnes nå i arkivet.
    registrer_fake_sak(&saksnummer);

    let svar = admin_retry(&env.nats_url, &[command.command_id]).await?;
    assert_ack(&svar);

    let events = wait_for_command_event(
        &env.nats_url,
        command.command_id,
        SkuffenCommandEvent::Fullfort,
        VENT,
    )
    .await?;
    assert!(
        events
            .iter()
            .any(|event| event.hendelse == SkuffenCommandEvent::Validert),
        "revalideringen publiserer ordinær status: {events:?}"
    );

    // Fullført arbeid gjentas ikke.
    let for_retry = hent_fullfort_command(&env.nats_url, command.command_id).await?;
    let svar = admin_retry(&env.nats_url, &[command.command_id]).await?;
    assert_ack(&svar);
    let etter_retry = hent_fullfort_command(&env.nats_url, command.command_id).await?;
    assert_eq!(
        etter_retry, for_retry,
        "retry skal ikke duplisere operasjoner"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn helt_avvist_loep_gjenopptas_via_saknokkel() -> Result<()> {
    helt_avvist_loep_gjenopptas(ADMIN_RETRY_SAK_SUBJECT).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn helt_avvist_loep_gjenopptas_via_kommandoliste() -> Result<()> {
    helt_avvist_loep_gjenopptas(ADMIN_RETRY_COMMANDS_SUBJECT).await
}

async fn helt_avvist_loep_gjenopptas(subject: &str) -> Result<()> {
    let env = support::start_runtime().await?;
    wait_for_admin_retry_responders(&env.nats_url).await?;
    let scenario = CommandScenario::new();
    publish_media(&env.nats_url, scenario.dokument_referanse).await?;
    let saksnummer = unikt_saksnummer();
    let sak_key =
        || -> Result<DtoSakKey> { Ok(DtoSakKey::ArkivId(DtoSaksnummer::new(&saksnummer)?)) };

    let mut loep = vec![
        scenario.sett_saksansvarlig(sak_key()?, "Z99999", "42"),
        scenario.opprett_skjermet_internt_notat("Z99999", "42", sak_key()?, "Avsluttet dokument"),
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::AvsluttSak(AvsluttSak {
                sak_key: sak_key()?,
            }),
        },
    ];
    let mut ids: Vec<Uuid> = loep.iter().map(|command| command.command_id).collect();
    ids.sort_unstable_by(|a, b| b.cmp(a));
    for (command, id) in loep.iter_mut().zip(&ids) {
        command.command_id = *id;
    }

    // Mottaksrekkefølgen er entydig og forskjellig fra sorterte command-id-er.
    for command in &loep {
        send_command_batch(&env.nats_url, std::slice::from_ref(command)).await?;
        let events = wait_for_command_event(
            &env.nats_url,
            command.command_id,
            SkuffenCommandEvent::Avvist,
            VENT,
        )
        .await?;
        assert_eq!(
            events.last().and_then(|event| event.error_code),
            Some(SkuffenStatusErrorCode::NotFound)
        );
    }

    let annet_saksnummer = saksnummer.replace("2026/", "2027/");
    let annen_command = CommandScenario::new().sett_saksansvarlig(
        DtoSakKey::ArkivId(DtoSaksnummer::new(&annet_saksnummer)?),
        "Z99999",
        "42",
    );
    send_command_batch(&env.nats_url, std::slice::from_ref(&annen_command)).await?;
    wait_for_command_event(
        &env.nats_url,
        annen_command.command_id,
        SkuffenCommandEvent::Avvist,
        VENT,
    )
    .await?;
    let annen_for = admin_hent_command(&env.nats_url, annen_command.command_id).await?;
    assert_eq!(annen_for["status"], "Ok", "{annen_for}");
    assert_eq!(annen_for["payload"]["operasjoner"], json!([]));

    let request = if subject == ADMIN_RETRY_SAK_SUBJECT {
        json!({
            "utfort_av": "test-operator",
            "key": { "type": "arkivId", "value": saksnummer }
        })
    } else {
        json!({ "utfort_av": "test-operator", "command_ids": ids })
    };
    let mut nye_events = Vec::new();
    for id in &ids {
        nye_events.push(nye_command_events(&env.nats_url, *id).await?);
    }
    let svar = admin_raw_request(&env.nats_url, subject, &request.to_string()).await?;
    assert_ack(&svar);
    for (consumer, id) in nye_events.iter().zip(&ids) {
        vent_pa_ny_avvisning(consumer, *id).await?;
    }

    registrer_fake_sak(&saksnummer);
    registrer_fake_sak(&annet_saksnummer);

    // En ordinær gjenutsendelse kvitteres, men dedupliseres på command_id.
    send_command_batch(&env.nats_url, &loep).await?;

    let svar = admin_raw_request(&env.nats_url, subject, &request.to_string()).await?;
    assert_ack(&svar);

    for id in &ids {
        let events =
            wait_for_command_event(&env.nats_url, *id, SkuffenCommandEvent::Fullfort, VENT).await?;
        let avvist = events
            .iter()
            .rposition(|event| event.hendelse == SkuffenCommandEvent::Avvist)
            .expect("avvist før retting");
        assert!(
            events[avvist + 1..]
                .iter()
                .any(|event| event.hendelse == SkuffenCommandEvent::Validert),
            "ordinær revalidering før fullføring: {events:?}"
        );
    }
    let annen_etter = admin_hent_command(&env.nats_url, annen_command.command_id).await?;
    assert_eq!(annen_etter, annen_for, "kun valgt sak skal gjenopptas");

    let mut for_retry = Vec::new();
    for id in &ids {
        for_retry.push(hent_fullfort_command(&env.nats_url, *id).await?);
    }
    let svar = admin_raw_request(&env.nats_url, subject, &request.to_string()).await?;
    assert_ack(&svar);
    for (id, snapshot) in ids.iter().zip(&for_retry) {
        let etter_retry = hent_fullfort_command(&env.nats_url, *id).await?;
        assert_eq!(
            etter_retry, *snapshot,
            "retry skal ikke duplisere operasjoner"
        );
    }
    let annen_etter = admin_hent_command(&env.nats_url, annen_command.command_id).await?;
    assert_eq!(annen_etter, annen_for, "kun valgt sak skal gjenopptas");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ukjente_og_ugyldige_foresporsler_gir_stabile_feilsvar() -> Result<()> {
    let env = support::start_runtime().await?;
    wait_for_admin_retry_responders(&env.nats_url).await?;

    let svar = admin_retry(&env.nats_url, &[Uuid::new_v4()]).await?;
    assert_eq!(svar["status"], "Error");
    assert_eq!(svar["payload"]["message"], "Command not found");

    let svar = admin_retry(&env.nats_url, &[]).await?;
    assert_eq!(svar["status"], "Error", "{svar}");
    assert_eq!(svar["payload"]["message"], "Invalid request format");

    for key in [
        json!({ "type": "skuffenId", "value": Uuid::new_v4() }),
        json!({ "type": "clientReference", "value": Uuid::new_v4() }),
        json!({ "type": "arkivId", "value": unikt_saksnummer() }),
    ] {
        let request = json!({ "utfort_av": "test-operator", "key": key });
        let svar =
            admin_raw_request(&env.nats_url, ADMIN_RETRY_SAK_SUBJECT, &request.to_string()).await?;
        assert_eq!(svar["status"], "Error", "{svar}");
        assert_eq!(svar["payload"]["message"], "Sak not found", "{svar}");
    }

    let key = json!({ "type": "arkivId", "value": unikt_saksnummer() });
    for ugyldig in [
        json!({ "command_ids": [Uuid::new_v4()] }),
        json!({ "key": key }),
        json!({ "utfort_av": "op", "command_ids": [Uuid::new_v4()], "sak": 1 }),
        json!({ "utfort_av": "op", "command_ids": [Uuid::new_v4()], "key": key }),
        json!({ "utfort_av": "op" }),
        json!({ "utfort_av": "op", "command_ids": null }),
        json!({ "utfort_av": "op", "key": null }),
        json!({ "utfort_av": "op", "command_ids": null, "key": key }),
        json!({ "utfort_av": "op", "command_ids": [Uuid::new_v4()], "key": null }),
        json!({ "utfort_av": "op", "command_ids": null, "key": null }),
    ] {
        for subject in [ADMIN_RETRY_SAK_SUBJECT, ADMIN_RETRY_COMMANDS_SUBJECT] {
            let svar = admin_raw_request(&env.nats_url, subject, &ugyldig.to_string()).await?;
            assert_eq!(
                svar["status"], "Error",
                "subject: {subject}, request: {ugyldig}, svar: {svar}"
            );
            assert_eq!(
                svar["payload"]["message"], "Invalid request format",
                "subject: {subject}, request: {ugyldig}, svar: {svar}"
            );
        }
    }

    for (subject, request) in [
        (
            ADMIN_RETRY_COMMANDS_SUBJECT,
            json!({ "utfort_av": "op", "key": key }),
        ),
        (
            ADMIN_RETRY_SAK_SUBJECT,
            json!({ "utfort_av": "op", "command_ids": [Uuid::new_v4()] }),
        ),
    ] {
        let svar = admin_raw_request(&env.nats_url, subject, &request.to_string()).await?;
        assert_eq!(svar["status"], "Error", "subject: {subject}, svar: {svar}");
        assert_eq!(
            svar["payload"]["message"], "Invalid request format",
            "subject: {subject}, svar: {svar}"
        );
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn alle_saknokler_finner_kjent_tom_sak_og_samme_avviste_kommando() -> Result<()> {
    let env = support::start_runtime().await?;
    wait_for_admin_retry_responders(&env.nats_url).await?;
    let pool = infrastructure::database::setup::setup_database().await?;
    let entiteter = PostgresEntitetRepository::new(pool);
    let client_reference = Uuid::new_v4();
    let saksnummer = unikt_saksnummer();
    let sak_id = entiteter
        .registrer(NyEntitet {
            skuffen_id: Uuid::new_v4(),
            entitet_type: EntitetType::Sak,
            client_reference: Some(client_reference),
            arkiv_id: Some(saksnummer.clone()),
        })
        .await?;
    let keys = [
        json!({ "type": "skuffenId", "value": sak_id }),
        json!({ "type": "clientReference", "value": client_reference }),
        json!({ "type": "arkivId", "value": saksnummer }),
    ];

    for key in &keys {
        let request = json!({ "utfort_av": "test-operator", "key": key });
        let svar =
            admin_raw_request(&env.nats_url, ADMIN_RETRY_SAK_SUBJECT, &request.to_string()).await?;
        assert_eq!(
            svar,
            json!({ "status": "Ok", "payload": null }),
            "key: {key}"
        );
    }

    let command = CommandScenario::new().sett_saksansvarlig(
        DtoSakKey::ArkivId(DtoSaksnummer::new(&saksnummer)?),
        "Z99999",
        "42",
    );
    send_command_batch(&env.nats_url, std::slice::from_ref(&command)).await?;
    wait_for_command_event(
        &env.nats_url,
        command.command_id,
        SkuffenCommandEvent::Avvist,
        VENT,
    )
    .await?;

    for key in &keys {
        let nye_events = nye_command_events(&env.nats_url, command.command_id).await?;
        let request = json!({ "utfort_av": "test-operator", "key": key });
        let svar =
            admin_raw_request(&env.nats_url, ADMIN_RETRY_SAK_SUBJECT, &request.to_string()).await?;
        assert_ack(&svar);
        vent_pa_ny_avvisning(&nye_events, command.command_id).await?;
    }

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gammelt_retry_subject_har_ingen_responder() -> Result<()> {
    let env = support::start_runtime().await?;
    wait_for_admin_retry_responders(&env.nats_url).await?;
    let client = async_nats::connect(&env.nats_url).await?;

    for request in [
        json!({ "utfort_av": "test-operator", "command_ids": [Uuid::new_v4()] }),
        json!({ "utfort_av": "test-operator", "key": { "type": "skuffenId", "value": Uuid::new_v4() } }),
    ] {
        let feil = tokio::time::timeout(
            Duration::from_secs(5),
            client.request(
                "arkiv.admin.write.command.retry",
                request.to_string().into(),
            ),
        )
        .await?
        .expect_err("gammelt retry-subject skal ikke ha et alias");
        assert_eq!(feil.kind(), async_nats::RequestErrorKind::NoResponders);
    }

    Ok(())
}
