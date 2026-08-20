use std::collections::HashSet;
use std::time::Duration;

use anyhow::Result;
use async_nats::jetstream;
use bytes::Bytes;
use futures::StreamExt;
use lib_nats::chunked_upload::protocol::{
    ChunkedUploadConfig, UploadMetadata, build_chunk_headers, split_payload,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::journalpost::JournalpostKey as DtoJournalpostKey;
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::query::queries::{HentJournalpostQuery, HentSakQuery};
use lib_schemas::skuffen::status::{SkuffenCommandEvent, SkuffenCommandStatusV1};
use tokio::time::Instant;

pub async fn publish_media(nats_url: &str, dokument_id: uuid::Uuid) -> Result<()> {
    let client = async_nats::connect(nats_url).await?;
    let payload = b"Skuffen testvedlegg".to_vec();
    let metadata = UploadMetadata {
        filename: Some("vedlegg.txt".to_string()),
        content_type: Some("text/plain".to_string()),
    };
    let config = ChunkedUploadConfig::default();
    let chunks = split_payload(&payload, config.chunk_size)?;
    let upload_id = dokument_id.to_string();
    let chunk_count = chunks.len() as u32;
    let total_size = payload.len();

    let inbox = client.new_inbox();
    let mut sub = client.subscribe(inbox.clone()).await?;

    for (index, chunk) in chunks.into_iter().enumerate() {
        let headers =
            build_chunk_headers(&upload_id, index as u32, chunk_count, total_size, &metadata);
        client
            .publish_with_reply_and_headers(
                "arkiv.arkiver.media",
                inbox.clone(),
                headers,
                Bytes::from(chunk),
            )
            .await?;
    }

    let message = tokio::time::timeout(Duration::from_secs(5), sub.next()).await?;
    let message = message.ok_or_else(|| anyhow::anyhow!("Missing media upload response"))?;
    let response_json: serde_json::Value = serde_json::from_slice(&message.payload)?;
    assert_eq!(
        response_json.get("status").and_then(|s| s.as_str()),
        Some("Ok")
    );
    assert_eq!(
        response_json.get("payload").and_then(|p| p.as_str()),
        Some(upload_id.as_str())
    );

    let store = jetstream::new(client)
        .get_object_store("arkiv_media")
        .await?;
    let _ = store.info(&upload_id).await?;
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
