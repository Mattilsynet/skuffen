use crate::command::media::MediaStore;
use crate::command::nats::kvittering::{self, Mediafeil, MediafeilArt};
use crate::command::wire_mapper::map_wire_envelope;
use crate::http::helse::Helse;
use crate::nats::client::NatsClient;
use crate::nats::supervisor::{RESTARTBUDSJETT, TaskSupervisor, tasknavn};
use application::command::services::ingest_command::{IngestCommandService, IngestFeil};
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

        let Some(reply_subject) = msg.reply.clone() else {
            error!("Command batch has no reply subject. Ignoring.");
            return;
        };

        let kvittering = self.motta_batch(&msg.payload).await;
        let payload = serde_json::to_vec(&kvittering).unwrap_or_default();
        let _ = self
            .client
            .inner()
            .publish(reply_subject, payload.into())
            .await;
    }

    async fn motta_batch(&self, payload: &[u8]) -> ArkiveringKvittering {
        let avvist = |message: String| ArkiveringKvittering::Error { message };

        let commands: Vec<CommandEnvelope<Command>> = match serde_json::from_slice(payload) {
            Ok(commands) => commands,
            Err(e) => {
                error!(
                    error_type = "deserialization",
                    payload_size = payload.len(),
                    "Failed to deserialize commands: {e}"
                );
                return avvist(kvittering::ugyldig_payload(&e));
            }
        };

        let navn: Vec<&'static str> = commands
            .iter()
            .map(|envelope| kvittering::kommandonavn(&envelope.payload))
            .collect();

        match self.validate_media(&commands).await {
            Ok(feil) if feil.is_empty() => {}
            Ok(feil) => {
                error!(antall_feil = feil.len(), "Media validation failed");
                return avvist(kvittering::manglende_media(&feil, &navn));
            }
            Err(err) => {
                error!(error = %format!("{err:#}"), "media lookup failed");
                return avvist(kvittering::INTERN_FEIL.to_string());
            }
        }

        let command_count = commands.len();
        let sequence = match CommandSequence::try_from(commands) {
            Ok(seq) => seq,
            Err(e) => {
                error!("Invalid command sequence: {e}");
                return avvist(kvittering::TOM_BATCH.to_string());
            }
        };

        Span::current().record("command_count", tracing::field::display(command_count));

        match self.ingest_sequence(sequence).await {
            Ok(command_ids) => {
                // Batchen er inngangen til alt som skjer videre. Uten id-ene
                // her finnes det ikke noe å slå opp løpet på.
                Span::current().record("command_ids", tracing::field::debug(&command_ids));
                info!(
                    command_count,
                    "kommandobatch mottatt og videresendt: {command_count} kommandoer"
                );
                ArkiveringKvittering::Ok { command_ids }
            }
            Err(IngestFeil::UgyldigBatch(feil)) => avvist(kvittering::ugyldig_batch(&feil, &navn)),
            Err(IngestFeil::Intern(e)) => {
                error!(command_count, error = %format!("{e:#}"), "kunne ikke ta imot kommandobatch");
                avvist(kvittering::INTERN_FEIL.to_string())
            }
        }
    }

    async fn ingest_sequence(
        &self,
        sequence: CommandSequence,
    ) -> Result<Vec<uuid::Uuid>, IngestFeil> {
        let commands: Vec<_> = sequence.into_iter().map(map_wire_envelope).collect();
        self.service.handle(commands).await
    }

    /// Feil i oppslaget mot lageret er Skuffens feil, ikke klientens, og
    /// returneres som `Err`.
    async fn validate_media(
        &self,
        commands: &[CommandEnvelope<Command>],
    ) -> anyhow::Result<Vec<Mediafeil>> {
        let mut feil = Vec::new();

        for (posisjon, envelope) in commands.iter().enumerate() {
            let dokumenter = match &envelope.payload {
                Command::OpprettInngåendeJournalpost(command) => &command.felles.dokumenter,
                Command::OpprettUtgåendeJournalpost(command) => &command.felles.dokumenter,
                Command::OpprettUtgåendeJournalpostMedUtsending(command) => {
                    &command.felles.dokumenter
                }
                Command::OpprettInterntNotatJournalpost(command) => &command.felles.dokumenter,
                Command::OpprettSak(_) | Command::AvsluttSak(_) | Command::SettSaksansvarlig(_) => {
                    continue;
                }
            };
            for (dokument, innhold) in dokumenter.iter().enumerate() {
                if let Some(art) = self.valider_dokument(innhold).await? {
                    feil.push(Mediafeil {
                        posisjon,
                        dokument,
                        art,
                    });
                }
            }
        }

        Ok(feil)
    }

    async fn valider_dokument(&self, dokument: &Dokument) -> anyhow::Result<Option<MediafeilArt>> {
        let (referanse, mangler) = match &dokument.form {
            Dokumentform::Bytes {
                dokument_referanse,
                filtype: _,
            } => (
                *dokument_referanse,
                MediafeilArt::DokumentIkkeLastetOpp {
                    dokument_referanse: *dokument_referanse,
                },
            ),
            Dokumentform::HtmlTemplate {
                mal_referanse,
                felter,
            } => {
                if !har_unike_felter(felter) {
                    return Ok(Some(MediafeilArt::DuplikateMalfelter));
                }
                (
                    *mal_referanse,
                    MediafeilArt::MalIkkeLastetOpp {
                        mal_referanse: *mal_referanse,
                    },
                )
            }
        };
        let finnes = self.media_store.exists(referanse).await?;
        Ok((!finnes).then_some(mangler))
    }
}

fn har_unike_felter(felter: &[Felt]) -> bool {
    let mut sett = HashSet::with_capacity(felter.len());
    felter.iter().all(|felt| sett.insert(*felt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tomme_malfelter_er_unike() {
        assert!(har_unike_felter(&[]));
    }

    #[test]
    fn ett_malfelt_er_unikt() {
        assert!(har_unike_felter(&[Felt::Saksnummer]));
    }

    #[test]
    fn samme_malfelt_to_ganger_er_ikke_unikt() {
        assert!(!har_unike_felter(&[Felt::Saksnummer, Felt::Saksnummer]));
    }
}
