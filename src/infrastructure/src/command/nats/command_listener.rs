use crate::command::media::MediaStore;
use crate::nats::client::NatsClient;
use crate::nats::nats_response::NatsResponse;
use crate::nats::supervisor::TaskSupervisor;
use application::command::services::ingest_command::IngestCommandService;
use async_nats::Message;
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope, CommandSequence};
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
            traceparent = tracing::field::Empty
        )
    )]
    async fn process_message(&self, msg: Message) {
        crate::telemetry::record_traceparent_from_headers(msg.headers.as_ref());
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
                error!("Failed to deserialize commands: {e}");
                let response = NatsResponse::<()>::Error {
                    message: format!("Invalid payload: {e}"),
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
                message: format!("Media validation failed: {err}"),
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
                    message: format!("Invalid sequence: {e}"),
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

        let traceparent = msg
            .headers
            .as_ref()
            .and_then(|headers| headers.get("traceparent"))
            .map(|traceparent| traceparent.as_str().to_owned());

        match self.ingest_sequence(sequence, traceparent).await {
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

    #[tracing::instrument(skip_all, name = "command.ingest", fields(traceparent = tracing::field::Empty))]
    async fn ingest_sequence(
        &self,
        sequence: CommandSequence,
        traceparent: Option<String>,
    ) -> anyhow::Result<()> {
        crate::telemetry::record_traceparent(traceparent.as_deref());
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
