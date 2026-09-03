use crate::command::media::MediaStore;
use crate::command::wire_mapper::map_wire_envelope;
use crate::http::helse::Helse;
use crate::nats::client::NatsClient;
use crate::nats::supervisor::{RESTARTBUDSJETT, TaskSupervisor, tasknavn};
use application::command::services::ingest_command::IngestCommandService;
use async_nats::Message;
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{
    ArkiveringKvittering, Command, CommandEnvelope, CommandSequence,
};
use lib_schemas::skuffen::dokument::{Dokument, Dokumentform, Felt};
use std::collections::HashSet;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, error, info};

pub struct CommandListener {
    client: NatsClient,
    service: IngestCommandService,
    media_store: std::sync::Arc<dyn MediaStore>,
    helse: Helse,
    shutdown: CancellationToken,
}

impl CommandListener {
    pub fn new(
        client: NatsClient,
        service: IngestCommandService,
        media_store: std::sync::Arc<dyn MediaStore>,
        helse: Helse,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            client,
            service,
            media_store,
            helse,
            shutdown,
        }
    }

    #[tracing::instrument(skip_all, name = "nats.command_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        TaskSupervisor::critical(tasknavn::COMMAND_LISTENER, RESTARTBUDSJETT)
            .with_shutdown(self.shutdown.clone())
            .with_helse(&self.helse)
            .run(|| self.run_once())
            .await
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

    /// Spanet lages her og får parent før det aktiveres. Settes parent fra
    /// innsiden av en `#[instrument]`-kropp, er OTel-spanet allerede bygget og
    /// avsenderens trace blir stille forkastet.
    async fn process_message(&self, msg: Message) {
        let span = tracing::info_span!(
            "nats.command_batch",
            subject = %msg.subject,
            reply_subject = ?msg.reply,
            command_count = tracing::field::Empty,
            command_ids = tracing::field::Empty,
        );
        crate::telemetry::set_parent_on_span_from_nats_headers(&span, msg.headers.as_ref());
        self.handle_message(msg).instrument(span).await
    }

    async fn handle_message(&self, msg: Message) {
        info!("mottok kommandobatch");

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
                let response = ArkiveringKvittering::Error {
                    message: "invalid payload format".to_string(),
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
            let response = ArkiveringKvittering::Error {
                message: "media validation failed".to_string(),
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
                let response = ArkiveringKvittering::Error {
                    message: "invalid command sequence".to_string(),
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
            Ok(command_ids) => {
                // Batchen er inngangen til alt som skjer videre. Uten id-ene
                // her finnes det ikke noe å slå opp løpet på.
                Span::current().record("command_ids", tracing::field::debug(&command_ids));
                info!(command_count, "kommandobatch mottatt og videresendt");
                let response = ArkiveringKvittering::Ok { command_ids };
                let payload = serde_json::to_vec(&response).unwrap_or_default();
                let _ = self
                    .client
                    .inner()
                    .publish(reply_subject, payload.into())
                    .await;
            }
            Err(e) => {
                error!(command_count, error = %e, "kunne ikke ta imot kommandobatch");
                let response = ArkiveringKvittering::Error {
                    message: "internal error".to_string(),
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

    async fn ingest_sequence(&self, sequence: CommandSequence) -> anyhow::Result<Vec<uuid::Uuid>> {
        let commands: Vec<_> = sequence.into_iter().map(map_wire_envelope).collect();
        self.service.handle(commands).await
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
                Command::OpprettUtgåendeJournalpostMedUtsending(command) => {
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
    let mut sett = HashSet::with_capacity(felter.len());
    for felt in felter {
        if !sett.insert(*felt) {
            return Err("HtmlTemplate har duplikate felter".to_string());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_html_template_felter_tillater_tomme_felter() {
        assert!(validate_html_template_felter(&[]).is_ok());
    }

    #[test]
    fn validate_html_template_felter_tillater_unike_felter() {
        assert!(validate_html_template_felter(&[Felt::Saksnummer]).is_ok());
    }

    #[test]
    fn validate_html_template_felter_avviser_duplikate_felter() {
        let err = validate_html_template_felter(&[Felt::Saksnummer, Felt::Saksnummer])
            .expect_err("duplicate felter should be rejected");

        assert_eq!(err, "HtmlTemplate har duplikate felter");
    }
}
