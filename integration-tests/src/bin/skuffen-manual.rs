use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use anyhow::Result;
use async_nats::{jetstream, Client, ConnectOptions};
use bytes::Bytes;
use futures::StreamExt;
use lib_nats::chunked_upload::protocol::{
    build_chunk_headers, split_payload, ChunkedUploadConfig, UploadMetadata,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::dokument::Dokument as DtoDokument;
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::status::SkuffenStatusEventV1;
use tokio::time::Instant;
use uuid::Uuid;

const DEFAULT_NATS_URL: &str = "nats://127.0.0.1:4222";
const RESOURCES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/resources");
const JOURNALPOST_PDF: &str = "dummy_journal_entry_report.pdf";
const ATTACHMENT_FOOD_SAFETY: &str = "attachment_food_safety_inspection.png";
const ATTACHMENT_ANIMAL_WELFARE: &str = "attachment_animal_welfare_inspection.png";

struct ManualAttachment {
    title: &'static str,
    filetype: &'static str,
    content_type: &'static str,
    path: PathBuf,
}

#[derive(Clone, Debug)]
struct ConnectionConfig {
    url: String,
    creds: Option<String>,
    context_name: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_crypto();
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        anyhow::bail!("Missing subcommand");
    }

    match args[1].as_str() {
        "ready" => {
            let config = resolve_connection_config(&args[2..])?;
            ready(&config).await
        }
        "upload-media" => {
            let (config, positional) = parse_connection_args(&args[2..])?;
            let dokument_referanse = positional
                .first()
                .ok_or_else(|| anyhow::anyhow!("Missing dokument_referanse UUID"))?;
            let dokument_referanse = Uuid::parse_str(dokument_referanse)?;
            let path = resource_path(JOURNALPOST_PDF)?;
            publish_media(&config, dokument_referanse, &path, "application/pdf").await?;
            println!("Uploaded media for dokument_referanse={dokument_referanse}");
            Ok(())
        }
        "send-sequence" => {
            let config = resolve_connection_config(&args[2..])?;
            send_sequence(&config).await
        }
        "watch-status" => {
            let (config, positional) = parse_connection_args(&args[2..])?;
            watch_status(&config, &positional).await
        }
        _ => {
            print_usage();
            anyhow::bail!("Unknown subcommand: {}", args[1]);
        }
    }
}

fn init_crypto() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .ok();
}

fn parse_connection_args(raw_args: &[String]) -> Result<(ConnectionConfig, Vec<String>)> {
    let mut context: Option<String> = None;
    let mut nats_url: Option<String> = None;
    let mut positional = Vec::new();

    let mut index = 0;
    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--context" => {
                let value = raw_args
                    .get(index + 1)
                    .ok_or_else(|| anyhow::anyhow!("--context requires a value"))?;
                context = Some(value.clone());
                index += 2;
            }
            "--nats-url" => {
                let value = raw_args
                    .get(index + 1)
                    .ok_or_else(|| anyhow::anyhow!("--nats-url requires a value"))?;
                nats_url = Some(value.clone());
                index += 2;
            }
            value => {
                positional.push(value.to_string());
                index += 1;
            }
        }
    }

    let config = if let Some(url) = nats_url {
        ConnectionConfig {
            url,
            creds: std::env::var("APP_NATS_CREDENTIALS")
                .ok()
                .and_then(|value| non_empty(value)),
            context_name: None,
        }
    } else {
        resolve_from_context_or_env(context.as_deref())
    };

    Ok((config, positional))
}

fn resolve_connection_config(raw_args: &[String]) -> Result<ConnectionConfig> {
    let (config, positional) = parse_connection_args(raw_args)?;
    if !positional.is_empty() {
        anyhow::bail!("Unexpected positional arguments: {}", positional.join(" "));
    }
    Ok(config)
}

fn resolve_from_context_or_env(context_name: Option<&str>) -> ConnectionConfig {
    if let Some(config) = context_name
        .and_then(|name| nats_context_config(Some(name)).ok())
        .or_else(|| nats_context_config(None).ok())
    {
        return config;
    }

    if let Some(url) = std::env::var("NATS_URL").ok().and_then(non_empty) {
        return ConnectionConfig {
            url,
            creds: std::env::var("APP_NATS_CREDENTIALS")
                .ok()
                .and_then(non_empty),
            context_name: None,
        };
    }

    ConnectionConfig {
        url: DEFAULT_NATS_URL.to_string(),
        creds: None,
        context_name: None,
    }
}

fn nats_context_config(context_name: Option<&str>) -> Result<ConnectionConfig> {
    let mut command = StdCommand::new("nats");
    command.args(["context", "info", "--json"]);
    if let Some(name) = context_name {
        command.arg(name);
    }

    let output = command.output()?;
    if !output.status.success() {
        anyhow::bail!("nats context info failed");
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let name = json
        .get("name")
        .and_then(|value| value.as_str())
        .and_then(|value| non_empty(value.to_string()));
    let url = json
        .get("url")
        .and_then(|value| value.as_str())
        .and_then(|value| non_empty(value.to_string()))
        .ok_or_else(|| anyhow::anyhow!("nats context has no url"))?;
    let creds = json
        .get("creds")
        .and_then(|value| value.as_str())
        .and_then(|value| non_empty(value.to_string()));

    Ok(ConnectionConfig {
        url,
        creds,
        context_name: name,
    })
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  skuffen-manual ready [--context NAME] [--nats-url URL]");
    eprintln!(
        "  skuffen-manual upload-media [--context NAME] [--nats-url URL] <DOKUMENT_REFERANSE_UUID>"
    );
    eprintln!("  skuffen-manual send-sequence [--context NAME] [--nats-url URL]");
    eprintln!(
        "  skuffen-manual watch-status [--context NAME] [--nats-url URL] <COMMAND_ID> [COMMAND_ID...]"
    );
}

async fn ready(config: &ConnectionConfig) -> Result<()> {
    let response = request_json(config, "skuffen.ready", &"ping").await?;
    if response.get("status").and_then(|value| value.as_str()) == Some("Ok") {
        println!("Skuffen ready on {}", config.url);
        return Ok(());
    }
    anyhow::bail!("Unexpected ready response: {response}");
}

async fn send_sequence(config: &ConnectionConfig) -> Result<()> {
    let saksbehandler_id = required_env("SIKRI_SAKSBEHANDLER_ID")?;
    let saksbehandler_enhet = required_env("SIKRI_SAKSBEHANDLER_ENHET")?;
    let attachments = manual_attachments()?;

    let sak_client_reference = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let dokumenter: Vec<(DtoDokument, Uuid, &'static str, PathBuf)> = attachments
        .into_iter()
        .map(|attachment| {
            let dokument_referanse = Uuid::new_v4();
            (
                DtoDokument {
                    client_reference: Uuid::new_v4(),
                    tittel: attachment.title.to_string(),
                    filtype: attachment.filetype.to_string(),
                    dokument_referanse,
                },
                dokument_referanse,
                attachment.content_type,
                attachment.path,
            )
        })
        .collect();

    for (dokument, dokument_referanse, content_type, path) in &dokumenter {
        publish_media(config, dokument_referanse.to_owned(), path, content_type).await?;
        println!(
            "Uploaded {} from {} as dokument_referanse={}",
            dokument.tittel,
            path.display(),
            dokument_referanse
        );
    }

    let correlation_id = Some(Uuid::new_v4());
    let command_sequence = vec![
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id,
            payload: Command::OpprettSak(OpprettSak {
                client_reference: sak_client_reference,
                sakstittel: lib_schemas::skuffen::sak::Sakstittel(format!(
                    "Skuffen manual test {}",
                    Uuid::new_v4()
                )),
                arkivdel: Arkivdel::Tilsynsdivisjonene,
                saksbehandler_id: saksbehandler_id.clone(),
                saksbehandler_enhet: saksbehandler_enhet.clone(),
                ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())?,
                tilgang: None,
            }),
        },
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id,
            payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
                felles: JournalpostCommon {
                    client_reference: journalpost_client_reference,
                    tittel: format!("Internt notat {}", Uuid::new_v4()),
                    dokument_dato: "2025-01-01".to_string(),
                    saksbehandler: saksbehandler_id.clone(),
                    saksbehandler_enhet: saksbehandler_enhet.clone(),
                    tilgang: None,
                    dokumenter: dokumenter
                        .iter()
                        .map(|(dokument, ..)| dokument.clone())
                        .collect(),
                    sak_key: DtoSakKey::ClientReference(sak_client_reference),
                    kildesystem: None,
                },
            }),
        },
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id,
            payload: Command::AvsluttSak(AvsluttSak {
                sak_key: DtoSakKey::ClientReference(sak_client_reference),
            }),
        },
    ];

    let response = request_json(config, "arkiv.arkiver", &command_sequence).await?;
    if response.get("status").and_then(|value| value.as_str()) != Some("Ok") {
        anyhow::bail!("Unexpected send response: {response}");
    }

    println!("Sent sequence to {}", config.url);
    println!("sak_client_reference={sak_client_reference}");
    println!("journalpost_client_reference={journalpost_client_reference}");
    println!("dokument_referanser:");
    for (dokument, dokument_referanse, _, path) in &dokumenter {
        println!(
            "- {} {} {}",
            dokument.tittel,
            dokument_referanse,
            path.display()
        );
    }
    println!("command_ids:");
    for command in &command_sequence {
        println!("- {}", command.command_id);
    }
    let command_ids: Vec<String> = command_sequence
        .iter()
        .map(|command| command.command_id.to_string())
        .collect();
    println!();
    println!("Copy and run this command to watch status history + live updates:");
    match config.context_name.as_deref() {
        Some(context) => println!(
            "cargo run -p skuffen-integration-tests --bin skuffen-manual -- watch-status --context {} {}",
            context,
            command_ids.join(" ")
        ),
        None => println!(
            "cargo run -p skuffen-integration-tests --bin skuffen-manual -- watch-status --nats-url {} {}",
            config.url,
            command_ids.join(" ")
        ),
    }
    Ok(())
}

async fn watch_status(config: &ConnectionConfig, command_ids: &[String]) -> Result<()> {
    if command_ids.is_empty() {
        print_usage();
        anyhow::bail!("watch-status requires at least one COMMAND_ID");
    }

    let ids: Vec<Uuid> = command_ids
        .iter()
        .map(|id| Uuid::parse_str(id))
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let client = connect_client(config).await?;
    let js = jetstream::new(client);
    let stream = js
        .get_or_create_stream(jetstream::stream::Config {
            name: "arkiv_status".to_string(),
            subjects: vec!["arkiv.status.*".to_string()],
            max_age: Duration::from_secs(60 * 60 * 24 * 180),
            ..Default::default()
        })
        .await?;

    // Track final archive IDs per command for the summary.
    struct CommandSummary {
        command_id: Uuid,
        correlation_id: Option<Uuid>,
        saksnummer: Option<String>,
        journalpost_id: Option<String>,
        dokument_ids: Vec<String>,
        final_status: Option<String>,
        final_message: Option<String>,
        error_code: Option<String>,
    }

    let mut summaries: Vec<CommandSummary> = ids
        .iter()
        .map(|&id| CommandSummary {
            command_id: id,
            correlation_id: None,
            saksnummer: None,
            journalpost_id: None,
            dokument_ids: vec![],
            final_status: None,
            final_message: None,
            error_code: None,
        })
        .collect();

    let timeout = Duration::from_secs(90);

    for summary in &mut summaries {
        let command_id = summary.command_id;
        println!();
        println!("── {command_id} ──────────────────────────────────────");

        let subject = format!("arkiv.status.{command_id}");
        let consumer = stream
            .create_consumer(jetstream::consumer::pull::Config {
                durable_name: None,
                ack_policy: jetstream::consumer::AckPolicy::Explicit,
                deliver_policy: jetstream::consumer::DeliverPolicy::All,
                filter_subject: subject,
                ..Default::default()
            })
            .await?;
        let mut messages = consumer.messages().await?;

        let deadline = Instant::now() + timeout;
        let mut terminal_seen = false;
        while !terminal_seen {
            let now = Instant::now();
            if now >= deadline {
                anyhow::bail!("Timed out waiting for status events for command_id={command_id}");
            }
            let wait_for = deadline
                .checked_duration_since(now)
                .unwrap_or_else(|| Duration::from_secs(0));
            let msg = tokio::time::timeout(wait_for, messages.next()).await?;
            let Some(msg) = msg else {
                anyhow::bail!("Status consumer closed for command_id={command_id}");
            };
            let msg = msg?;
            let event: SkuffenStatusEventV1 = serde_json::from_slice(&msg.payload)?;

            // Accumulate archive IDs — later events may fill them in.
            if let Some(ref c) = event.correlation_id {
                summary.correlation_id = Some(*c);
            }
            if let Some(ref s) = event.saksnummer {
                summary.saksnummer = Some(s.as_str().to_string());
            }
            if let Some(ref jp) = event.journalpost_id {
                summary.journalpost_id = Some(jp.0.clone());
            }
            if let Some(ref docs) = event.dokument_id {
                summary.dokument_ids = docs.iter().map(|d| d.0.clone()).collect();
            }
            if event.terminal {
                summary.final_status = Some(format!("{:?}", event.status));
                summary.final_message = Some(event.message.clone());
                summary.error_code = event.error_code.as_ref().map(|e| format!("{e:?}"));
            }

            // Print this event line.
            let ts = event.timestamp.as_deref().unwrap_or("-");
            let attempt = event
                .attempt
                .map(|v| format!("attempt={v}"))
                .unwrap_or_default();
            let error = event
                .error_code
                .as_ref()
                .map(|e| format!("  error_code={e:?}"))
                .unwrap_or_default();
            let terminal_marker = if event.terminal { " ✓" } else { "" };
            println!(
                "  [{ts}] {phase:?} › {status:?}{terminal_marker}  {attempt}  {msg_text}{error}",
                phase = event.phase,
                status = event.status,
                msg_text = event.message,
            );

            // Print archive IDs inline as soon as they arrive.
            if event.saksnummer.is_some()
                || event.journalpost_id.is_some()
                || event.dokument_id.is_some()
            {
                if let Some(ref s) = event.saksnummer {
                    println!("    saksnummer     = {}", s.as_str());
                }
                if let Some(ref jp) = event.journalpost_id {
                    println!("    journalpost_id = {}", jp.0);
                }
                if let Some(ref docs) = event.dokument_id {
                    for doc in docs {
                        println!("    dokument_id    = {}", doc.0);
                    }
                }
            }

            msg.ack()
                .await
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
            terminal_seen = event.terminal;
        }
    }

    // ── Final summary ─────────────────────────────────────────────────────
    println!();
    println!("══════════════════════════════════════════════════════");
    println!("  SUMMARY");
    println!("══════════════════════════════════════════════════════");
    for s in &summaries {
        println!();
        println!("  command_id     = {}", s.command_id);
        if let Some(ref c) = s.correlation_id {
            println!("  correlation_id = {c}");
        }
        if let Some(ref status) = s.final_status {
            let msg = s.final_message.as_deref().unwrap_or("");
            println!("  status         = {status}  ({msg})");
        }
        if let Some(ref e) = s.error_code {
            println!("  error_code     = {e}");
        }
        if let Some(ref s) = s.saksnummer {
            println!("  saksnummer     = {s}");
        }
        if let Some(ref jp) = s.journalpost_id {
            println!("  journalpost_id = {jp}");
        }
        for doc in &s.dokument_ids {
            println!("  dokument_id    = {doc}");
        }
    }
    println!();
    println!("All tracked command IDs reached terminal status.");
    Ok(())
}

async fn publish_media(
    config: &ConnectionConfig,
    dokument_id: Uuid,
    path: &Path,
    content_type: &str,
) -> Result<()> {
    let client = connect_client(config).await?;
    let payload = fs::read(path)?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid filename for {}", path.display()))?;
    let metadata = UploadMetadata {
        filename: Some(filename.to_string()),
        content_type: Some(content_type.to_string()),
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
    if response_json.get("status").and_then(|s| s.as_str()) != Some("Ok") {
        anyhow::bail!("Media upload failed: {response_json}");
    }

    Ok(())
}

fn manual_attachments() -> Result<Vec<ManualAttachment>> {
    Ok(vec![
        ManualAttachment {
            title: "Dummy journal entry report",
            filetype: "PDF",
            content_type: "application/pdf",
            path: resource_path(JOURNALPOST_PDF)?,
        },
        ManualAttachment {
            title: "Attachment food safety inspection",
            filetype: "PNG",
            content_type: "image/png",
            path: resource_path(ATTACHMENT_FOOD_SAFETY)?,
        },
        ManualAttachment {
            title: "Attachment animal welfare inspection",
            filetype: "PNG",
            content_type: "image/png",
            path: resource_path(ATTACHMENT_ANIMAL_WELFARE)?,
        },
    ])
}

fn resource_path(filename: &str) -> Result<PathBuf> {
    let path = Path::new(RESOURCES_DIR).join(filename);
    if !path.exists() {
        anyhow::bail!("Missing resource file: {}", path.display());
    }
    Ok(path)
}

async fn request_json<T: serde::Serialize>(
    config: &ConnectionConfig,
    subject: &str,
    payload: &T,
) -> Result<serde_json::Value> {
    let body = serde_json::to_vec(payload)?;
    let client = connect_client(config).await?;
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

async fn connect_client(config: &ConnectionConfig) -> Result<Client> {
    let mut options = ConnectOptions::new();
    if let Some(creds) = config.creds.as_deref() {
        let creds_path = std::path::Path::new(creds);
        if creds_path.exists() {
            options = options.credentials_file(creds_path).await?;
        } else {
            options = options.credentials(creds)?;
        }
    }
    let client = options.connect(&config.url).await?;
    Ok(client)
}

fn required_env(name: &str) -> Result<String> {
    let value = std::env::var(name)
        .map_err(|_| anyhow::anyhow!("Environment variable {name} must be set"))?;
    non_empty(value)
        .ok_or_else(|| anyhow::anyhow!("Environment variable {name} must be set and non-empty"))
}
