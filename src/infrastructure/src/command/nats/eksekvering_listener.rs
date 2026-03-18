use async_nats::jetstream::{self, AckKind, consumer};
use futures::StreamExt;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use tracing::{Instrument, error, info};

use crate::command::adapter::id_mapping_postgres::PostgresIdMappingRepository;
use crate::nats::client::NatsClient;
use application::command::ports::eksekvering_state_port::EksekveringStateRepository;
use application::command::ports::eksekvering_state_port::{JournalpostState, SakState, SakStatus};
use application::command::ports::id_mapping_port::IdMappingRepository;
use domain::eksekvering::plan::{EksekveringsPlan, JournalpostType, Steg, Utsending};
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;

pub struct KommandoEksekveringListener {
    client: NatsClient,
    state_repo: Box<dyn EksekveringStateRepository>,
    id_mapping_repo: PostgresIdMappingRepository,
}

impl KommandoEksekveringListener {
    pub fn new(
        client: NatsClient,
        state_repo: Box<dyn EksekveringStateRepository>,
        id_mapping_repo: PostgresIdMappingRepository,
    ) -> Self {
        Self {
            client,
            state_repo,
            id_mapping_repo,
        }
    }

    #[tracing::instrument(skip_all, name = "nats.execution_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        let jetstream = jetstream::new(self.client.inner().clone());
        let stream = match jetstream
            .get_or_create_stream(jetstream::stream::Config {
                name: "arkiv_command_ready".to_string(),
                subjects: vec!["arkiv.command.ready.>".to_string()],
                max_age: std::time::Duration::from_secs(60 * 60 * 24 * 180),
                ..Default::default()
            })
            .await
        {
            Ok(stream) => stream,
            Err(err) => return Err(anyhow::anyhow!("JetStream stream error: {err}")),
        };

        let consumer = match stream
            .get_or_create_consumer(
                "executor",
                consumer::pull::Config {
                    durable_name: Some("executor".to_string()),
                    ack_policy: consumer::AckPolicy::Explicit,
                    ..Default::default()
                },
            )
            .await
        {
            Ok(consumer) => consumer,
            Err(err) => return Err(anyhow::anyhow!("JetStream consumer create error: {err}")),
        };

        let mut messages = match consumer.messages().await {
            Ok(messages) => messages,
            Err(err) => return Err(anyhow::anyhow!("JetStream consumer error: {err}")),
        };

        while let Some(message) = messages.next().await {
            let message = match message {
                Ok(msg) => msg,
                Err(err) => {
                    error!("JetStream error: {err}");
                    continue;
                }
            };

            let envelope: CommandEnvelope<Command> = match serde_json::from_slice(&message.payload)
            {
                Ok(cmd) => cmd,
                Err(err) => {
                    error!("Failed to deserialize command: {err}");
                    if let Err(err) = message.ack().await {
                        error!("Ack failed: {err}");
                    }
                    continue;
                }
            };

            let span = tracing::info_span!(
                "command.register_execution",
                command_id = %envelope.command_id,
                correlation_id = ?envelope.correlation_id,
                traceparent = tracing::field::Empty
            );
            if let Some(headers) = message.headers.as_ref()
                && let Some(parent) = headers.get("traceparent")
            {
                span.record("traceparent", tracing::field::display(parent.as_str()));
            }
            let result = async {
                self.ensure_sak_state(&envelope).await?;
                self.state_repo.registrer_kommando(&envelope).await
            }
            .instrument(span)
            .await;
            match result {
                Ok(()) => {
                    if let Err(err) = message.ack().await {
                        error!("Ack failed: {err}");
                    }
                }
                Err(err) => {
                    info!("Kunne ikke lagre kommando: {err}");
                    if let Err(err) = message.ack_with(AckKind::Nak(None)).await {
                        error!("NAK failed: {err}");
                    }
                }
            }
        }

        Ok(())
    }

    async fn ensure_sak_state(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error> {
        let plan = EksekveringsPlan::fra_command(&envelope.payload)
            .map_err(|err| anyhow::anyhow!(err.melding))?;

        if let Some(Steg::OpprettJournalpost { plan }) = plan.steg.first() {
            let (sak_id, sak_state) = match &plan.sak_key {
                DtoSakKey::ClientReference(sak_id) => (
                    *sak_id,
                    SakState {
                        status: SakStatus::UnderBehandling,
                        opprettet: false,
                        saksnummer: None,
                    },
                ),
                DtoSakKey::ArkivId(saksnummer) => {
                    let sak_id = self
                        .id_mapping_repo
                        .ensure_arkiv_mapping("sak", saksnummer.as_str())
                        .await?;
                    (
                        sak_id,
                        SakState {
                            status: SakStatus::UnderBehandling,
                            opprettet: true,
                            saksnummer: Some(saksnummer.as_str().to_string()),
                        },
                    )
                }
            };

            if self.state_repo.hent_sak_state(sak_id).await?.is_none() {
                self.state_repo.lagre_sak_state(sak_id, sak_state).await?;
            }

            if self
                .state_repo
                .hent_journalpost_state(plan.journalpost_id)
                .await?
                .is_none()
            {
                let journalposttype = match plan.journalpost_type {
                    JournalpostType::Inngaende => 'I',
                    JournalpostType::Utgaaende => 'U',
                    JournalpostType::InterntNotat => 'X',
                };
                let journalpost_state = JournalpostState {
                    journalfoert: false,
                    avskrevet: false,
                    ekspedert: false,
                    har_feilede_dokumenter: false,
                    med_utsending: matches!(plan.utsending, Some(Utsending::MedUtsending)),
                    journalposttype,
                    journalpostnummer: None,
                };
                self.state_repo
                    .lagre_journalpost_state(plan.journalpost_id, sak_id, journalpost_state)
                    .await?;
            }
        }

        Ok(())
    }
}
