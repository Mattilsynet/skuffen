use std::collections::HashSet;
use std::time::Duration;

use anyhow::Result;
use async_nats::jetstream;
use bytes::Bytes;
use futures::StreamExt;
use lib_nats::chunked_upload::protocol::{
    build_chunk_headers, split_payload, ChunkedUploadConfig, UploadMetadata,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::journalpost::JournalpostKey as DtoJournalpostKey;
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::query::queries::{HentJournalpostQuery, HentSakQuery};
use lib_schemas::skuffen::status::SkuffenStatusEventV1;
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
) -> Result<Vec<SkuffenStatusEventV1>> {
    let mut pending: HashSet<uuid::Uuid> = command_ids.into_iter().collect();
    if pending.is_empty() {
        return Ok(Vec::new());
    }

    let client = async_nats::connect(nats_url).await?;
    let jetstream = jetstream::new(client);
    let stream = jetstream
        .get_or_create_stream(jetstream::stream::Config {
            name: "arkiv_status".to_string(),
            subjects: vec!["arkiv.status.*".to_string()],
            max_age: Duration::from_secs(60 * 60 * 24 * 180),
            ..Default::default()
        })
        .await?;
    let consumer = stream
        .create_consumer(jetstream::consumer::pull::Config {
            durable_name: None,
            ack_policy: jetstream::consumer::AckPolicy::Explicit,
            deliver_policy: jetstream::consumer::DeliverPolicy::All,
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
        let event: SkuffenStatusEventV1 = serde_json::from_slice(&message.payload)?;
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
) -> Result<()> {
    let payload = serde_json::to_vec(commands)?;
    let client = async_nats::connect(nats_url).await?;
    let inbox = client.new_inbox();
    let mut sub = client.subscribe(inbox.clone()).await?;
    client
        .publish_with_reply("arkiv.arkiver", inbox, Bytes::from(payload))
        .await?;
    let response = tokio::time::timeout(Duration::from_secs(5), sub.next()).await?;
    let response = response.ok_or_else(|| anyhow::anyhow!("Missing command batch response"))?;
    let response_json: serde_json::Value = serde_json::from_slice(&response.payload)?;
    assert_eq!(
        response_json.get("status").and_then(|s| s.as_str()),
        Some("Ok")
    );
    Ok(())
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
        if let Some(msg) = probe_sub.next().await {
            if let Some(reply) = msg.reply {
                let _ = client_for_reply
                    .publish(reply, Bytes::from_static(b"pong"))
                    .await;
            }
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

pub async fn hent_sak_via_nats(
    nats_url: &str,
    skuffen_id: uuid::Uuid,
) -> Result<serde_json::Value> {
    let query = HentSakQuery {
        key: DtoSakKey::ClientReference(skuffen_id),
    };
    request_via_nats(nats_url, "sak.hent", &query).await
}

pub async fn hent_journalpost_via_nats(
    nats_url: &str,
    journalpost_id: uuid::Uuid,
) -> Result<serde_json::Value> {
    let query = HentJournalpostQuery {
        key: DtoJournalpostKey::ClientReference(journalpost_id),
    };
    request_via_nats(nats_url, "journalpost.hent", &query).await
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

// request_nats removed; queries use handlers directly in tests.
