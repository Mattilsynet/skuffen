use crate::command::media::MediaStore;
use crate::nats::client::NatsClient;
use crate::nats::nats_response::NatsResponse;
use application::command::services::ingest_command::IngestCommandService;
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope, CommandSequence};
use tracing::{error, info, Instrument};

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
        let subject = "arkiv.arkiver";
        info!("Listening for command batches on '{}'", subject);

        // Queue group 'skuffen-command-processor' for load balancing if scaled
        let mut sub = self
            .client
            .inner()
            .queue_subscribe(subject.to_string(), "skuffen-command-processor".to_string())
            .await?;

        while let Some(msg) = sub.next().await {
            let span = tracing::info_span!(
                "nats.command_batch",
                subject = %msg.subject,
                reply_subject = ?msg.reply,
                command_count = tracing::field::Empty,
                traceparent = tracing::field::Empty
            );
            if let Some(headers) = msg.headers.as_ref() {
                if let Some(parent) = headers.get("traceparent") {
                    span.record(
                        "traceparent",
                        tracing::field::display(parent.as_str()),
                    );
                }
            }
            let _guard = span.enter();
            info!("Received command batch");

            let reply_subject = match msg.reply.clone() {
                Some(r) => r,
                None => {
                    error!("Command batch has no reply subject. Ignoring.");
                    continue;
                }
            };

            // Deserialize Vec<CommandEnvelope<Command>>
            let commands: Vec<CommandEnvelope<Command>> = match serde_json::from_slice(&msg.payload)
            {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to deserialize commands: {e}");
                    // Reply error
                    let response = NatsResponse::<()>::Error {
                        message: format!("Invalid payload: {e}"),
                    };
                    let payload = serde_json::to_vec(&response).unwrap_or_default();
                    let _ = self
                        .client
                        .inner()
                        .publish(reply_subject, payload.into())
                        .await;
                    continue;
                }
            };

            if let Err(err) = self.validate_media(&commands).await {
                error!("Media validation failed: {err}");
                let response = NatsResponse::<()>::Error {
                    message: format!("Media validation failed: {err}"),
                };
                let payload = serde_json::to_vec(&response).unwrap_or_default();
                let _ = self
                    .client
                    .inner()
                    .publish(reply_subject, payload.into())
                    .await;
                continue;
            }

            // Validate sequence (Infrastructure responsibility: Parse/Validate input structure)
            let command_count = commands.len();
            let sequence = match CommandSequence::try_from(commands) {
                Ok(seq) => seq,
                Err(e) => {
                    error!("Invalid command sequence: {e}");
                    let response = NatsResponse::<()>::Error {
                        message: format!("Invalid sequence: {e}"),
                    };
                    let payload = serde_json::to_vec(&response).unwrap_or_default();
                    let _ = self
                        .client
                        .inner()
                        .publish(reply_subject, payload.into())
                        .await;
                    continue;
                }
            };

            span.record("command_count", tracing::field::display(command_count));

            // Ingest
            let handle_span = tracing::info_span!(
                "command.ingest",
                traceparent = tracing::field::Empty
            );
            if let Some(headers) = msg.headers.as_ref() {
                if let Some(parent) = headers.get("traceparent") {
                    handle_span.record(
                        "traceparent",
                        tracing::field::display(parent.as_str()),
                    );
                }
            }
            match self.service.handle(sequence).instrument(handle_span).await {
                Ok(_) => {
                    // Reply OK
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
                        message: e.to_string(),
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
        Ok(())
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
                Command::OpprettSak(_) | Command::AvsluttSak(_) => {}
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
        dokumenter: &[lib_schemas::skuffen::dokument::Dokument],
        missing: &mut Vec<String>,
    ) -> Result<(), String> {
        for dokument in dokumenter {
            let id = dokument.dokument_referanse;
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
