use crate::command::media::MediaStore;
use crate::nats::client::NatsClient;
use crate::nats::nats_response::NatsResponse;
use crate::nats::supervisor::TaskSupervisor;
use application::command::services::ingest_command::IngestCommandService;
use async_nats::Message;
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope, CommandSequence};
use lib_schemas::skuffen::dokument::{Dokument, Dokumentform, Felt};
use std::collections::HashSet;
use tracing::{Span, error, info};

pub struct CommandListener {
    client: NatsClient,
    service: IngestCommandService,
    media_store: std::sync::Arc<dyn MediaStore>,
}

impl CommandListener {
    pub fn new(
        client: NatsClient,
        service: IngestCommandService,
        media_store: std::sync::Arc<dyn MediaStore>,
    ) -> Self {
        Self {
            client,
            service,
            media_store,
        }
    }

    #[tracing::instrument(skip_all, name = "nats.command_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        let supervisor = TaskSupervisor::critical("command_listener", 3);
        supervisor.run(|| self.run_once()).await
    }

    async fn run_once(&self) -> anyhow::Result<()> {
        let subject = "arkiv.arkiver";
        info!("Listening for command batches on '{}'", subject);

        // Queue group 'skuffen-command-processor' for load balancing if scaled
        let mut sub = self
            .client
            .inner()
            .queue_subscribe(subject.to_string(), "skuffen-command-processor".to_string())
            .await?;

        while let Some(msg) = sub.next().await {
            self.process_message(msg).await;
        }

        Err(anyhow::anyhow!(
            "command listener subscription ended unexpectedly"
        ))
    }

    #[tracing::instrument(
        skip_all,
        name = "nats.command_batch",
        fields(
            subject = %msg.subject,
            reply_subject = ?msg.reply,
            command_count = tracing::field::Empty,
        )
    )]
    async fn process_message(&self, msg: Message) {
        crate::telemetry::set_parent_from_nats_headers(msg.headers.as_ref());
        info!("Received command batch");

        let reply_subject = match msg.reply.clone() {
            Some(r) => r,
            None => {
                error!("Command batch has no reply subject. Ignoring.");
                return;
            }
        };

        let commands: Vec<CommandEnvelope<Command>> = match serde_json::from_slice(&msg.payload) {
            Ok(c) => c,
            Err(e) => {
                error!(
                    error_type = "deserialization",
                    payload_size = msg.payload.len(),
                    "Failed to deserialize commands: {e}"
                );
                let response = NatsResponse::<()>::Error {
                    message: "Invalid payload format".to_string(),
                };
                let payload = serde_json::to_vec(&response).unwrap_or_default();
                let _ = self
                    .client
                    .inner()
                    .publish(reply_subject, payload.into())
                    .await;
                return;
            }
        };

        if let Err(err) = self.validate_media(&commands).await {
            error!("Media validation failed: {err}");
            let response = NatsResponse::<()>::Error {
                message: "Media validation failed".to_string(),
            };
            let payload = serde_json::to_vec(&response).unwrap_or_default();
            let _ = self
                .client
                .inner()
                .publish(reply_subject, payload.into())
                .await;
            return;
        }

        let command_count = commands.len();
        let sequence = match CommandSequence::try_from(commands) {
            Ok(seq) => seq,
            Err(e) => {
                error!("Invalid command sequence: {e}");
                let response = NatsResponse::<()>::Error {
                    message: "Invalid command sequence".to_string(),
                };
                let payload = serde_json::to_vec(&response).unwrap_or_default();
                let _ = self
                    .client
                    .inner()
                    .publish(reply_subject, payload.into())
                    .await;
                return;
            }
        };

        Span::current().record("command_count", tracing::field::display(command_count));

        match self.ingest_sequence(sequence).await {
            Ok(()) => {
                let response = NatsResponse::Ok(());
                let payload = serde_json::to_vec(&response).unwrap_or_default();
                let _ = self
                    .client
                    .inner()
                    .publish(reply_subject, payload.into())
                    .await;
            }
            Err(e) => {
                error!("Failed to process commands: {e}");
                let response = NatsResponse::<()>::Error {
                    message: "Internal error".to_string(),
                };
                let payload = serde_json::to_vec(&response).unwrap_or_default();
                let _ = self
                    .client
                    .inner()
                    .publish(reply_subject, payload.into())
                    .await;
            }
        }
    }

    #[tracing::instrument(skip_all, name = "command.ingest")]
    async fn ingest_sequence(&self, sequence: CommandSequence) -> anyhow::Result<()> {
        self.service.handle(sequence).await
    }

    async fn validate_media(&self, commands: &[CommandEnvelope<Command>]) -> Result<(), String> {
        let mut missing: Vec<String> = Vec::new();

        for envelope in commands {
            match &envelope.payload {
                Command::OpprettInngåendeJournalpost(command) => {
                    self.validate_media_references(&command.felles.dokumenter, &mut missing)
                        .await?;
                }
                Command::OpprettUtgåendeJournalpost(command) => {
                    self.validate_media_references(&command.felles.dokumenter, &mut missing)
                        .await?;
                }
                Command::OpprettInterntNotatJournalpost(command) => {
                    self.validate_media_references(&command.felles.dokumenter, &mut missing)
                        .await?;
                }
                Command::OpprettSak(_) | Command::AvsluttSak(_) | Command::SettSaksansvarlig(_) => {
                }
            }
        }

        if missing.is_empty() {
            return Ok(());
        }

        let list = missing.join(", ");
        Err(format!("Missing media: {list}"))
    }

    async fn validate_media_references(
        &self,
        dokumenter: &[Dokument],
        missing: &mut Vec<String>,
    ) -> Result<(), String> {
        for dokument in dokumenter {
            let id = match &dokument.form {
                Dokumentform::Bytes {
                    dokument_referanse,
                    filtype: _,
                } => *dokument_referanse,
                Dokumentform::HtmlTemplate {
                    mal_referanse,
                    felter,
                } => {
                    validate_html_template_felter(felter)?;
                    *mal_referanse
                }
            };
            let exists = self
                .media_store
                .exists(id)
                .await
                .map_err(|err| err.to_string())?;
            if !exists {
                missing.push(id.to_string());
            }
        }
        Ok(())
    }
}

fn validate_html_template_felter(felter: &[Felt]) -> Result<(), String> {
    if felter.is_empty() {
        return Err("HtmlTemplate må deklarere minst ett felt".to_string());
    }

    let mut sett = HashSet::with_capacity(felter.len());
    for felt in felter {
        if !sett.insert(*felt) {
            return Err("HtmlTemplate har duplikate felter".to_string());
        }
    }

    Ok(())
}
