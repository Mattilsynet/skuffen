use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;

use anyhow::Result;
use async_nats::{Client, ConnectOptions, jetstream};
use bytes::Bytes;
use futures::StreamExt;
use lib_nats::chunked_upload::protocol::{
    ChunkedUploadConfig, UploadMetadata, build_chunk_headers, split_payload,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak, SettSaksansvarlig};
use lib_schemas::skuffen::dokument::{Dokument as DtoDokument, Dokumentform, Felt};
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;
use lib_schemas::skuffen::status::{SkuffenCommandEvent, SkuffenCommandStatusV1};
use lib_schemas::skuffen::tilgang::{Tilgangshjemmel, Tilgangskode, Tilgjengelighet};
use tokio::time::Instant;
use uuid::Uuid;

const DEFAULT_NATS_URL: &str = "nats://127.0.0.1:4222";
const RESOURCES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/resources");
const JOURNALPOST_PDF: &str = "dummy_journal_entry_report.pdf";
const ATTACHMENT_FOOD_SAFETY: &str = "attachment_food_safety_inspection.png";
const ATTACHMENT_ANIMAL_WELFARE: &str = "attachment_animal_welfare_inspection.png";
const INTERNAL_NOTE_TEMPLATE: &str = "internal_note_template.html";
const HTML_TEMPLATE_CONTENT_TYPE: &str = "text/html; charset=utf-8";
const DEFAULT_WATCH_STATUS_TIMEOUT_SECONDS: u64 = 30;

struct ManualAttachment {
    title: &'static str,
    filetype: &'static str,
    content_type: &'static str,
    path: PathBuf,
}

#[derive(Clone)]
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
                .and_then(non_empty),
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
        "  skuffen-manual watch-status [--context NAME] [--nats-url URL] [--timeout-seconds SECONDS] <COMMAND_ID> [COMMAND_ID...]"
    );
}

async fn ready(config: &ConnectionConfig) -> Result<()> {
    let response = request_json(config, "skuffen.ready", &"ping").await?;
    if response.get("status").and_then(|value| value.as_str()) == Some("Ok") {
        println!("Skuffen ready on {}", redact_url_userinfo(&config.url));
        return Ok(());
    }
    anyhow::bail!("Unexpected ready response: {response}");
}

async fn send_sequence(config: &ConnectionConfig) -> Result<()> {
    let saksbehandler_id = required_env("SIKRI_SAKSBEHANDLER_ID")?;
    let saksbehandler_enhet = required_env("SIKRI_SAKSBEHANDLER_ENHET")?;
    let skjerming_tilgjengelighet = manual_skjerming_tilgjengelighet();
    print_skjerming_tilgjengelighet_warning(&skjerming_tilgjengelighet);
    let attachments = manual_attachments()?;

    let sak_client_reference = Uuid::new_v4();
    let shielded_sak_client_reference = Uuid::new_v4();
    let bytes_journalpost_client_reference = Uuid::new_v4();
    let shielded_journalpost_client_reference = Uuid::new_v4();
    let shielded_dokument_client_reference = Uuid::new_v4();
    let shielded_dokument_referanse = Uuid::new_v4();
    let template_journalpost_client_reference = Uuid::new_v4();
    let template_dokument_client_reference = Uuid::new_v4();
    let mal_referanse = Uuid::new_v4();
    let dokumenter: Vec<(DtoDokument, Uuid, &'static str, PathBuf)> = attachments
        .into_iter()
        .map(|attachment| {
            let dokument_referanse = Uuid::new_v4();
            (
                DtoDokument {
                    client_reference: Uuid::new_v4(),
                    tittel: attachment.title.to_string(),
                    form: Dokumentform::Bytes {
                        filtype: attachment.filetype.to_string(),
                        dokument_referanse,
                    },
                },
                dokument_referanse,
                attachment.content_type,
                attachment.path,
            )
        })
        .collect();

    let shielded_dokument_path = resource_path(JOURNALPOST_PDF)?;
    let shielded_dokument = DtoDokument {
        client_reference: shielded_dokument_client_reference,
        tittel: "[Skjermet journalpost hoveddokument tittel fra Skuffen Test.]".to_string(),
        form: Dokumentform::Bytes {
            filtype: "PDF".to_string(),
            dokument_referanse: shielded_dokument_referanse,
        },
    };

    for (dokument, dokument_referanse, content_type, path) in &dokumenter {
        publish_media(config, dokument_referanse.to_owned(), path, content_type).await?;
        println!(
            "Uploaded {} from {} as dokument_referanse={}",
            dokument.tittel,
            path.display(),
            dokument_referanse
        );
    }

    publish_media(
        config,
        shielded_dokument_referanse,
        &shielded_dokument_path,
        "application/pdf",
    )
    .await?;
    println!(
        "Uploaded {} from {} as dokument_referanse={}",
        shielded_dokument.tittel,
        shielded_dokument_path.display(),
        shielded_dokument_referanse
    );

    let template_path = resource_path(INTERNAL_NOTE_TEMPLATE)?;
    publish_media(
        config,
        mal_referanse,
        &template_path,
        HTML_TEMPLATE_CONTENT_TYPE,
    )
    .await?;
    println!(
        "Uploaded HTML template from {} as mal_referanse={}",
        template_path.display(),
        mal_referanse
    );

    let correlation_id = Some(Uuid::new_v4());
    let command_sequence = vec![
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id,
            payload: Command::OpprettSak(OpprettSak {
                client_reference: sak_client_reference,
                sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                    "Skuffen manual test {}",
                    Uuid::new_v4()
                ))
                .unwrap(),
                arkivdel: Arkivdel::Tilsynsdivisjonene,
                saksbehandler_id: saksbehandler_id.clone(),
                saksbehandler_enhet: saksbehandler_enhet.clone(),
                ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())?,
                tilgjengelighet: Tilgjengelighet::Offentlig,
            }),
        },
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id,
            payload: Command::OpprettSak(OpprettSak {
                client_reference: shielded_sak_client_reference,
                sakstittel: lib_schemas::skuffen::sak::Sakstittel::try_from(format!(
                    "[|Ola Norrmann|] - Skuffen manual test {}",
                    Uuid::new_v4()
                ))
                .unwrap(),
                arkivdel: Arkivdel::Tilsynsdivisjonene,
                saksbehandler_id: saksbehandler_id.clone(),
                saksbehandler_enhet: saksbehandler_enhet.clone(),
                ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())?,
                tilgjengelighet: skjerming_tilgjengelighet.clone(),
            }),
        },
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id,
            payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
                felles: JournalpostCommon {
                    client_reference: shielded_journalpost_client_reference,
                    tittel: format!("[|Ola Norrmann|] - Internt notat {}", Uuid::new_v4()),
                    dokument_dato: "2025-01-01".to_string(),
                    saksbehandler: saksbehandler_id.clone(),
                    saksbehandler_enhet: saksbehandler_enhet.clone(),
                    tilgjengelighet: skjerming_tilgjengelighet.clone(),
                    dokumenter: vec![shielded_dokument.clone()],
                    sak_key: DtoSakKey::ClientReference(shielded_sak_client_reference),
                    kildesystem: None,
                },
            }),
        },
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id,
            payload: Command::SettSaksansvarlig(SettSaksansvarlig {
                sak_key: DtoSakKey::ClientReference(shielded_sak_client_reference),
                saksbehandler_id: saksbehandler_id.clone(),
                saksbehandler_enhet: saksbehandler_enhet.clone(),
            }),
        },
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id,
            payload: Command::AvsluttSak(AvsluttSak {
                sak_key: DtoSakKey::ClientReference(shielded_sak_client_reference),
            }),
        },
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id,
            payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
                felles: JournalpostCommon {
                    client_reference: bytes_journalpost_client_reference,
                    tittel: format!("Internt notat {}", Uuid::new_v4()),
                    dokument_dato: "2025-01-01".to_string(),
                    saksbehandler: saksbehandler_id.clone(),
                    saksbehandler_enhet: saksbehandler_enhet.clone(),
                    tilgjengelighet: Tilgjengelighet::Offentlig,
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
            payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
                felles: JournalpostCommon {
                    client_reference: template_journalpost_client_reference,
                    tittel: format!("Internt notat med HTML-mal {}", Uuid::new_v4()),
                    dokument_dato: "2025-01-01".to_string(),
                    saksbehandler: saksbehandler_id.clone(),
                    saksbehandler_enhet: saksbehandler_enhet.clone(),
                    tilgjengelighet: Tilgjengelighet::Offentlig,
                    dokumenter: vec![DtoDokument {
                        client_reference: template_dokument_client_reference,
                        tittel: "HTML-template notat".to_string(),
                        form: Dokumentform::HtmlTemplate {
                            mal_referanse,
                            felter: vec![Felt::Saksnummer],
                        },
                    }],
                    sak_key: DtoSakKey::ClientReference(sak_client_reference),
                    kildesystem: None,
                },
            }),
        },
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id,
            payload: Command::SettSaksansvarlig(SettSaksansvarlig {
                sak_key: DtoSakKey::ClientReference(sak_client_reference),
                saksbehandler_id: saksbehandler_id.clone(),
                saksbehandler_enhet: saksbehandler_enhet.clone(),
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

    let expected_command_ids: Vec<Uuid> = command_sequence
        .iter()
        .map(|command| command.command_id)
        .collect();
    let command_roles = [
        "OpprettSak",
        "OpprettSak(skjermet)",
        "OpprettInterntNotatJournalpost(skjermet)",
        "SettSaksansvarlig(skjermet)",
        "AvsluttSak(skjermet)",
        "OpprettInterntNotatJournalpost(bytes)",
        "OpprettInterntNotatJournalpost(html-template)",
        "SettSaksansvarlig",
        "AvsluttSak",
    ];
    assert_eq!(command_roles.len(), command_sequence.len());

    let response = request_json(config, "arkiv.arkiver", &command_sequence).await?;
    let ok_response = response
        .get("Ok")
        .ok_or_else(|| anyhow::anyhow!("Unexpected send response: {response}"))?;
    let command_ids: Vec<Uuid> = serde_json::from_value(ok_response["command_ids"].clone())?;
    if command_ids != expected_command_ids {
        anyhow::bail!("Unexpected command ids in send response: {response}");
    }

    println!("Sent sequence to {}", redact_url_userinfo(&config.url));
    println!("sak_client_reference={sak_client_reference}");
    println!("shielded_sak_client_reference={shielded_sak_client_reference}");
    println!("bytes_journalpost_client_reference={bytes_journalpost_client_reference}");
    println!("shielded_journalpost_client_reference={shielded_journalpost_client_reference}");
    println!("shielded_dokument_client_reference={shielded_dokument_client_reference}");
    println!("template_journalpost_client_reference={template_journalpost_client_reference}");
    println!("template_dokument_client_reference={template_dokument_client_reference}");
    println!("mal_referanse={mal_referanse}");
    println!("dokument_referanser:");
    for (dokument, dokument_referanse, _, path) in &dokumenter {
        println!(
            "- {} {} {}",
            dokument.tittel,
            dokument_referanse,
            path.display()
        );
    }
    println!("shielded_dokument_referanse:");
    println!(
        "- {} {} {}",
        shielded_dokument.tittel,
        shielded_dokument_referanse,
        shielded_dokument_path.display()
    );
    println!("command_ids:");
    for (role, command) in command_roles.iter().zip(&command_sequence) {
        println!("- {role}: {}", command.command_id);
    }
    let command_ids: Vec<String> = command_sequence
        .iter()
        .map(|command| command.command_id.to_string())
        .collect();
    println!();
    println!(
        "HTML-template rendering requires SKUFFEN_HTML2PDF_RENDERER_ENDPOINT in the deployed Skuffen environment."
    );
    println!("Copy and run this command to watch status history + live updates:");
    match config.context_name.as_deref() {
        Some(context) => println!(
            "cargo run -p skuffen-integration-tests --bin skuffen-manual -- watch-status --context {} {}",
            context,
            command_ids.join(" ")
        ),
        None => println!(
            "cargo run -p skuffen-integration-tests --bin skuffen-manual -- watch-status --nats-url {} {}",
            redact_url_userinfo(&config.url),
            command_ids.join(" ")
        ),
    }
    Ok(())
}

fn manual_skjerming_tilgjengelighet() -> Tilgjengelighet {
    // Disse defaultverdiene må finnes i target Sikri-kodeverk for
    // TILGANGSKODE/TILGANGSHJEMMEL før sekvensen kjøres mot et miljø.
    Tilgjengelighet::Skjermet {
        tilgangskode: Tilgangskode::new("UO").expect("gyldig tilgangskode"),
        tilgangshjemmel: Tilgangshjemmel::new("Offl. § 23 tredje ledd")
            .expect("gyldig tilgangshjemmel"),
    }
}

fn print_skjerming_tilgjengelighet_warning(tilgjengelighet: &Tilgjengelighet) {
    if let Tilgjengelighet::Skjermet {
        tilgangskode,
        tilgangshjemmel,
    } = tilgjengelighet
    {
        eprintln!(
            "Shielded title coverage uses synthetic [|Ola Norrmann|] titles with environment-specific default tilgangskode={} and tilgangshjemmel={}. These values are not validated by this tool; confirm they exist in the target Sikri code sets before running against real archive data.",
            tilgangskode.as_str(),
            tilgangshjemmel.as_str()
        );
    }
}

fn redact_url_userinfo(url: &str) -> String {
    let authority_start = url.find("://").map_or(0, |scheme_end| scheme_end + 3);
    let authority = &url[authority_start..];
    let authority_end = authority
        .char_indices()
        .find_map(|(index, ch)| matches!(ch, '/' | '?' | '#').then_some(index))
        .unwrap_or(authority.len());
    let authority = &authority[..authority_end];

    let Some(at_index) = authority.rfind('@') else {
        return url.to_string();
    };

    let credentials_end = authority_start + at_index + 1;
    format!(
        "{}<redacted>@{}",
        &url[..authority_start],
        &url[credentials_end..]
    )
}

fn parse_watch_status_args(raw_args: &[String]) -> Result<(Vec<Uuid>, Duration)> {
    let mut timeout_seconds = DEFAULT_WATCH_STATUS_TIMEOUT_SECONDS;
    let mut command_ids = Vec::new();

    let mut index = 0;
    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--timeout-seconds" => {
                let value = raw_args
                    .get(index + 1)
                    .ok_or_else(|| anyhow::anyhow!("--timeout-seconds requires a value"))?;
                timeout_seconds = value.parse::<u64>().map_err(|err| {
                    anyhow::anyhow!("Invalid --timeout-seconds value {value:?}: {err}")
                })?;
                if timeout_seconds == 0 {
                    anyhow::bail!("--timeout-seconds must be greater than zero");
                }
                index += 2;
            }
            value => {
                command_ids.push(Uuid::parse_str(value)?);
                index += 1;
            }
        }
    }

    Ok((command_ids, Duration::from_secs(timeout_seconds)))
}

async fn watch_status(config: &ConnectionConfig, raw_args: &[String]) -> Result<()> {
    let (ids, timeout) = parse_watch_status_args(raw_args)?;
    if ids.is_empty() {
        print_usage();
        anyhow::bail!("watch-status requires at least one COMMAND_ID");
    }

    let client = connect_client(config).await?;
    let js = jetstream::new(client);
    let stream = js
        .get_or_create_stream(jetstream::stream::Config {
            name: "arkiv_status".to_string(),
            subjects: vec!["arkiv.status.>".to_string()],
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
        reached_terminal_ok: bool,
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
            reached_terminal_ok: false,
        })
        .collect();

    let deadline = Instant::now() + timeout;

    for summary in &mut summaries {
        let command_id = summary.command_id;
        println!();
        println!("── {command_id} ──────────────────────────────────────");

        let subject = format!("arkiv.status.{command_id}.command");
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

        let mut terminal_seen = false;
        while !terminal_seen {
            let now = Instant::now();
            if now >= deadline {
                summary.final_status = Some("TimedOut".to_string());
                summary.final_message = Some(format!(
                    "Timed out after {} total seconds waiting for terminal status event",
                    timeout.as_secs()
                ));
                break;
            }
            let wait_for = deadline
                .checked_duration_since(now)
                .unwrap_or_else(|| Duration::from_secs(0));
            let msg = match tokio::time::timeout(wait_for, messages.next()).await {
                Ok(msg) => msg,
                Err(_) => {
                    summary.final_status = Some("TimedOut".to_string());
                    summary.final_message = Some(format!(
                        "Timed out after {} total seconds waiting for terminal status event",
                        timeout.as_secs()
                    ));
                    break;
                }
            };
            let Some(msg) = msg else {
                summary.final_status = Some("ConsumerClosed".to_string());
                summary.final_message =
                    Some("Status consumer closed before terminal event".to_string());
                break;
            };
            let msg = match msg {
                Ok(msg) => msg,
                Err(err) => {
                    summary.final_status = Some("ConsumerError".to_string());
                    summary.final_message = Some(format!("Status consumer error: {err}"));
                    summary.error_code = Some("status_consumer_error".to_string());
                    break;
                }
            };
            let event: SkuffenCommandStatusV1 = match serde_json::from_slice(&msg.payload) {
                Ok(event) => event,
                Err(err) => {
                    summary.final_status = Some("InvalidStatusPayload".to_string());
                    summary.final_message = Some(format!("Failed to parse status payload: {err}"));
                    summary.error_code = Some("invalid_status_payload".to_string());
                    break;
                }
            };

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
            if let Some(ref docs) = event.dokument_client_references {
                summary.dokument_ids = docs.iter().map(|d| d.to_string()).collect();
            }
            if event.terminal {
                summary.reached_terminal_ok = event.hendelse == SkuffenCommandEvent::Fullfort;
                summary.final_status = Some(format!("{:?}", event.hendelse));
                summary.final_message = Some(event.message.clone());
                summary.error_code = event.error_code.as_ref().map(|e| format!("{e:?}"));
            }

            // Print this event line.
            let ts = event.timestamp.as_deref().unwrap_or("-");
            let error = event
                .error_code
                .as_ref()
                .map(|e| format!("  error_code={e:?}"))
                .unwrap_or_default();
            let terminal_marker = if event.terminal { " ✓" } else { "" };
            println!(
                "  [{ts}] {hendelse:?}{terminal_marker}  {msg_text}{error}",
                hendelse = event.hendelse,
                msg_text = event.message,
            );

            // Print archive IDs inline as soon as they arrive.
            if event.saksnummer.is_some()
                || event.journalpost_id.is_some()
                || event.dokument_client_references.is_some()
            {
                if let Some(ref s) = event.saksnummer {
                    println!("    saksnummer     = {}", s.as_str());
                }
                if let Some(ref jp) = event.journalpost_id {
                    println!("    journalpost_id = {}", jp.0);
                }
                if let Some(ref docs) = event.dokument_client_references {
                    for doc in docs {
                        println!("    dokument_ref   = {doc}");
                    }
                }
            }

            if let Err(err) = msg.ack().await {
                summary.reached_terminal_ok = false;
                summary.final_status = Some("AckFailed".to_string());
                summary.final_message = Some(format!("Failed to ack status message: {err}"));
                summary.error_code = Some("status_ack_failed".to_string());
                break;
            }
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
    let failure_count = summaries
        .iter()
        .filter(|summary| !summary.reached_terminal_ok)
        .count();
    if failure_count > 0 {
        anyhow::bail!("{failure_count} tracked command ID(s) did not reach terminal Ok status");
    }

    println!("All tracked command IDs reached terminal Ok status.");
    Ok(())
}

async fn publish_media(
    config: &ConnectionConfig,
    upload_reference: Uuid,
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
    let upload_id = upload_reference.to_string();
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
            options = options
                .credentials(creds)
                .map_err(|_| anyhow::anyhow!("invalid APP_NATS_CREDENTIALS inline credentials"))?;
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
