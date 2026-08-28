use std::collections::HashSet;
use std::time::Duration;

use anyhow::Result;
use async_nats::jetstream;
use bytes::Bytes;
use futures::StreamExt;
use lib_nats::chunked_upload::{ChunkedUploadClient, ChunkedUploadClientConfig, UploadRequest};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::journalpost::JournalpostKey as DtoJournalpostKey;
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::query::queries::{HentJournalpostQuery, HentSakQuery};
use lib_schemas::skuffen::status::{SkuffenCommandEvent, SkuffenCommandStatusV1};
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::time::Instant;

pub async fn publish_media(nats_url: &str, dokument_id: uuid::Uuid) -> Result<()> {
    let client = async_nats::connect(nats_url).await?;
    let payload: Vec<u8> = (0..2_000_001).map(|index| (index % 251) as u8).collect();
    let upload_id = dokument_id.to_string();
    let uploader = ChunkedUploadClient::new(
        client.clone(),
        ChunkedUploadClientConfig {
            base_subject: "arkiv.arkiver.media".to_string(),
            ..ChunkedUploadClientConfig::default()
        },
    );
    let receipt = uploader
        .upload(UploadRequest {
            upload_id: upload_id.clone(),
            bytes: Bytes::from(payload.clone()),
            filename: Some("vedlegg.txt".to_string()),
            content_type: Some("text/plain".to_string()),
        })
        .await?;
    assert_eq!(receipt.upload_id, upload_id);

    let store = jetstream::new(client)
        .get_object_store("arkiv_media")
        .await?;
    let mut object = store.get(&upload_id).await?;
    let mut stored = Vec::new();
    object.read_to_end(&mut stored).await?;
    assert_eq!(stored, payload);
    Ok(())
}

pub async fn wait_for_status_events(
    nats_url: &str,
    command_ids: impl IntoIterator<Item = uuid::Uuid>,
    timeout: Duration,
) -> Result<Vec<SkuffenCommandStatusV1>> {
    let mut pending: HashSet<uuid::Uuid> = command_ids.into_iter().collect();
    if pending.is_empty() {
        return Ok(Vec::new());
    }

    let client = async_nats::connect(nats_url).await?;
    let jetstream = jetstream::new(client);
    let stream = jetstream
        .get_or_create_stream(jetstream::stream::Config {
            name: "arkiv_status".to_string(),
            subjects: vec!["arkiv.status.>".to_string()],
            max_age: Duration::from_secs(60 * 60 * 24 * 180),
            ..Default::default()
        })
        .await?;
    let consumer = stream
        .create_consumer(jetstream::consumer::pull::Config {
            durable_name: None,
            ack_policy: jetstream::consumer::AckPolicy::Explicit,
            deliver_policy: jetstream::consumer::DeliverPolicy::All,
            // Bare command_outcomeet. Operasjonsdetaljer ligger ett nivå
            // dypere, på `arkiv.status.<cmd>.operasjon.<id>`.
            filter_subject: "arkiv.status.*.command".to_string(),
            ..Default::default()
        })
        .await?;
    let mut messages = consumer.messages().await?;

    let deadline = Instant::now() + timeout;
    let mut events = Vec::new();
    while !pending.is_empty() {
        let now = Instant::now();
        if now >= deadline {
            anyhow::bail!("Timed out waiting for status events");
        }
        let wait_for = deadline
            .checked_duration_since(now)
            .unwrap_or_else(|| Duration::from_secs(0));
        let message = tokio::time::timeout(wait_for, messages.next()).await?;
        let Some(message) = message else {
            anyhow::bail!("Timed out waiting for status events");
        };
        let message = message?;
        let event: SkuffenCommandStatusV1 = serde_json::from_slice(&message.payload)?;
        if pending.contains(&event.command_id) {
            let terminal = event.terminal;
            let command_id = event.command_id;
            events.push(event);
            if terminal {
                pending.remove(&command_id);
            }
        }
        message
            .ack()
            .await
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    }

    Ok(events)
}

pub async fn send_command_batch(
    nats_url: &str,
    commands: &[CommandEnvelope<Command>],
) -> Result<Vec<uuid::Uuid>> {
    let payload = serde_json::to_vec(commands)?;
    let command_ids: Vec<uuid::Uuid> = commands.iter().map(|c| c.command_id).collect();
    let client = async_nats::connect(nats_url).await?;
    let inbox = client.new_inbox();
    let mut sub = client.subscribe(inbox.clone()).await?;
    client
        .publish_with_reply("arkiv.arkiver", inbox, Bytes::from(payload))
        .await?;
    let response = tokio::time::timeout(Duration::from_secs(5), sub.next()).await?;
    let response = response.ok_or_else(|| anyhow::anyhow!("Missing command batch response"))?;
    let response_json: serde_json::Value = serde_json::from_slice(&response.payload)?;

    // Command replies bruker `ArkiveringKvittering`, ikke `NatsResponse<T>`.
    let ok_variant = response_json.get("Ok").ok_or_else(|| {
        anyhow::anyhow!("Expected Ok variant in response, got: {:?}", response_json)
    })?;
    let command_ids_response: Vec<uuid::Uuid> =
        serde_json::from_value(ok_variant["command_ids"].clone())?;

    // Assert returned command_ids match submitted command ids in order
    assert_eq!(
        command_ids_response, command_ids,
        "Returned command_ids do not match submitted command ids"
    );

    Ok(command_ids_response)
}

/// Send en rå JSON-payload til `arkiv.arkiver` og returner kvitteringen uendret.
///
/// Brukes for adversarisk testing: de validerte wire-typene kan ikke konstruere
/// ugyldige verdier i Rust, så vi må sende rå bytes for å simulere en feilaktig
/// eller ondsinnet klient.
pub async fn send_raw_command_payload(nats_url: &str, raw_json: &str) -> Result<serde_json::Value> {
    let client = async_nats::connect(nats_url).await?;
    let inbox = client.new_inbox();
    let mut sub = client.subscribe(inbox.clone()).await?;
    client
        .publish_with_reply(
            "arkiv.arkiver",
            inbox,
            Bytes::from(raw_json.as_bytes().to_vec()),
        )
        .await?;
    let response = tokio::time::timeout(Duration::from_secs(5), sub.next()).await?;
    let response = response.ok_or_else(|| anyhow::anyhow!("Missing command batch response"))?;
    let kvittering: serde_json::Value = serde_json::from_slice(&response.payload)?;
    Ok(kvittering)
}

pub async fn wait_for_ready(nats_url: &str) -> Result<()> {
    let payload = serde_json::to_vec("ping")?;
    let client = async_nats::connect(nats_url).await?;
    let inbox = client.new_inbox();
    let mut sub = client.subscribe(inbox.clone()).await?;
    client
        .publish_with_reply("skuffen.ready", inbox, Bytes::from(payload))
        .await?;
    let response = tokio::time::timeout(Duration::from_secs(5), sub.next()).await?;
    let response = response.ok_or_else(|| anyhow::anyhow!("Missing ready response"))?;
    if response.status == Some(async_nats::StatusCode::NO_RESPONDERS) || response.payload.is_empty()
    {
        anyhow::bail!("No responders for skuffen.ready");
    }
    let response_json: serde_json::Value = serde_json::from_slice(&response.payload)?;
    assert_eq!(
        response_json.get("status").and_then(|s| s.as_str()),
        Some("Ok")
    );
    Ok(())
}

pub async fn wait_for_nats_ready(nats_url: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        match nats_server_ping(nats_url).await {
            Ok(()) => return Ok(()),
            Err(err) => {
                if Instant::now() >= deadline {
                    return Err(err);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

async fn nats_server_ping(nats_url: &str) -> Result<()> {
    let client = async_nats::connect(nats_url).await?;

    let probe_subject = format!("nats.ready.probe.{}", uuid::Uuid::new_v4());
    let mut probe_sub = client.subscribe(probe_subject.clone()).await?;
    let client_for_reply = client.clone();

    let responder = tokio::spawn(async move {
        if let Some(msg) = probe_sub.next().await
            && let Some(reply) = msg.reply
        {
            let _ = client_for_reply
                .publish(reply, Bytes::from_static(b"pong"))
                .await;
        }
    });

    let inbox = client.new_inbox();
    let mut response_sub = client.subscribe(inbox.clone()).await?;
    client
        .publish_with_reply(probe_subject, inbox, Bytes::from_static(b"ping"))
        .await?;
    let response = tokio::time::timeout(Duration::from_secs(5), response_sub.next()).await?;
    let response = response.ok_or_else(|| anyhow::anyhow!("Missing NATS ping response"))?;
    if response.payload.is_empty() {
        anyhow::bail!("NATS request/reply probe returned empty payload");
    }
    let _ = responder.await;
    Ok(())
}

#[allow(dead_code)]
pub async fn hent_sak_via_nats(
    nats_url: &str,
    skuffen_id: uuid::Uuid,
) -> Result<serde_json::Value> {
    let query = HentSakQuery {
        key: DtoSakKey::ClientReference(skuffen_id),
    };
    request_via_nats(nats_url, "arkiv.request.sak.hent", &query).await
}

pub async fn hent_journalpost_via_nats(
    nats_url: &str,
    journalpost_id: uuid::Uuid,
) -> Result<serde_json::Value> {
    let query = HentJournalpostQuery {
        key: DtoJournalpostKey::ClientReference(journalpost_id),
    };
    request_via_nats(nats_url, "arkiv.request.journalpost.hent", &query).await
}

pub async fn hent_bruker_mt_enheter_via_nats(nats_url: &str) -> Result<serde_json::Value> {
    // Query replies forblir pakket som `NatsResponse<T>`.
    request_via_nats(
        nats_url,
        "arkiv.request.bruker.mt_enheter",
        &serde_json::json!({}),
    )
    .await
}

async fn request_via_nats<T: serde::Serialize>(
    nats_url: &str,
    subject: &str,
    payload: &T,
) -> Result<serde_json::Value> {
    let body = serde_json::to_vec(payload)?;
    let client = async_nats::connect(nats_url).await?;
    let inbox = client.new_inbox();
    let mut sub = client.subscribe(inbox.clone()).await?;
    client
        .publish_with_reply(subject.to_string(), inbox, Bytes::from(body))
        .await?;
    let response = tokio::time::timeout(Duration::from_secs(5), sub.next()).await?;
    let response = response.ok_or_else(|| anyhow::anyhow!("Missing NATS response"))?;
    if response.status == Some(async_nats::StatusCode::NO_RESPONDERS) || response.payload.is_empty()
    {
        anyhow::bail!("No responders for {subject}");
    }
    let response_json: serde_json::Value = serde_json::from_slice(&response.payload)?;
    Ok(response_json)
}

pub fn extract_saksnummer(
    events: &[SkuffenCommandStatusV1],
    command_id: uuid::Uuid,
) -> Option<String> {
    events
        .iter()
        .find(|e| {
            e.command_id == command_id && e.terminal && e.hendelse == SkuffenCommandEvent::Fullfort
        })
        .and_then(|e| e.saksnummer.as_ref().map(|s| s.as_str().to_string()))
}

pub async fn hent_sak_via_nats_by_arkiv_id(
    nats_url: &str,
    saksnummer: &str,
) -> Result<serde_json::Value> {
    let query = HentSakQuery {
        key: DtoSakKey::ArkivId(lib_schemas::skuffen::sak::Saksnummer::new(saksnummer)?),
    };
    request_via_nats(nats_url, "arkiv.request.sak.hent", &query).await
}

// ---------------------------------------------------------------------------
// Admin read
// ---------------------------------------------------------------------------

pub const ADMIN_COMMAND_SUBJECT: &str = "arkiv.admin.read.command.hent";
pub const ADMIN_SAK_SUBJECT: &str = "arkiv.admin.read.sak.hent";

pub async fn admin_hent_command(nats_url: &str, command_id: uuid::Uuid) -> Result<Value> {
    admin_raw_request(
        nats_url,
        ADMIN_COMMAND_SUBJECT,
        &serde_json::json!({ "utfort_av": "test-operator", "command_id": command_id }).to_string(),
    )
    .await
}

pub async fn admin_hent_sak(nats_url: &str, key: Value) -> Result<Value> {
    admin_raw_request(
        nats_url,
        ADMIN_SAK_SUBJECT,
        &serde_json::json!({ "utfort_av": "test-operator", "key": key }).to_string(),
    )
    .await
}

/// Rå payload, slik at også ugyldige requester kan sendes.
pub async fn admin_raw_request(nats_url: &str, subject: &str, raw_json: &str) -> Result<Value> {
    let svar =
        admin_raw_request_alle_svar(nats_url, subject, raw_json, Duration::from_millis(0)).await?;
    svar.into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("Missing admin response on {subject}"))
}

/// Samler alle svar på en rå inbox, og venter et stille vindu etter første svar
/// for å avkrefte et svar nummer to.
pub async fn admin_raw_request_alle_svar(
    nats_url: &str,
    subject: &str,
    raw_json: &str,
    stille_vindu: Duration,
) -> Result<Vec<Value>> {
    let client = async_nats::connect(nats_url).await?;
    let inbox = client.new_inbox();
    let mut sub = client.subscribe(inbox.clone()).await?;
    client
        .publish_with_reply(
            subject.to_string(),
            inbox,
            Bytes::from(raw_json.as_bytes().to_vec()),
        )
        .await?;
    client.flush().await?;

    let forste = tokio::time::timeout(Duration::from_secs(5), sub.next()).await?;
    let forste = forste.ok_or_else(|| anyhow::anyhow!("Missing admin response on {subject}"))?;
    if forste.status == Some(async_nats::StatusCode::NO_RESPONDERS) {
        anyhow::bail!("No responders for {subject}");
    }

    let mut svar = vec![serde_json::from_slice::<Value>(&forste.payload)?];
    if !stille_vindu.is_zero() {
        while let Ok(Some(melding)) = tokio::time::timeout(stille_vindu, sub.next()).await {
            svar.push(serde_json::from_slice::<Value>(&melding.payload)?);
        }
    }
    Ok(svar)
}

/// Begge admin-subjectene må ha responder. Forventet `not found` teller.
pub async fn wait_for_admin_responders(nats_url: &str) -> Result<()> {
    let command = admin_hent_command(nats_url, uuid::Uuid::new_v4()).await?;
    anyhow::ensure!(
        command["status"] == "Error" || command["status"] == "Ok",
        "uventet admin command-svar: {command}"
    );
    let sak = admin_hent_sak(
        nats_url,
        serde_json::json!({ "type": "skuffenId", "value": uuid::Uuid::new_v4() }),
    )
    .await?;
    anyhow::ensure!(
        sak["status"] == "Error" || sak["status"] == "Ok",
        "uventet admin sak-svar: {sak}"
    );
    Ok(())
}

/// Venter til NATS rapporterer `antall` køemedlemmer på subjectet.
///
/// En vanlig request kan bare bevise at minst én responder finnes; queue
/// group-testen må først vite at begge faktisk er subscribet.
pub async fn wait_for_queue_members(
    monitor_url: &str,
    subject: &str,
    queue_group: &str,
    antall: usize,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let subsz: Value = reqwest::get(format!("{monitor_url}/subsz?subs=1"))
            .await?
            .json()
            .await?;
        let treff = subsz["subscriptions_list"]
            .as_array()
            .map(|liste| {
                liste
                    .iter()
                    .filter(|sub| sub["subject"] == subject && sub["qgroup"] == queue_group)
                    .count()
            })
            .unwrap_or(0);
        if treff >= antall {
            return Ok(());
        }
        if Instant::now() >= deadline {
            anyhow::bail!("fant bare {treff} køemedlemmer på {subject}, forventet {antall}");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
