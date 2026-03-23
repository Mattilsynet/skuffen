use anyhow::Result;
use lib_schemas::skuffen::command::commands::{
    Command, CommandEnvelope, CommandStatus, CommandStatusEvent,
};
use lib_schemas::skuffen::query::queries::SakKey;

use crate::command::ports::{
    command_state_port::ArkivSakTilstandRepository, id_mapping_port::IdMappingRepository,
    status_publisher_port::CommandStatusPublisher,
    validated_command_dispatcher_port::ValidatedCommandDispatcher,
};

pub enum ValidationOutcome {
    Ok,
    Blocked { message: String },
    Recoverable { message: String },
    Irrecoverable { message: String },
}

impl ValidationOutcome {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ValidationOutcome::Recoverable { .. } | ValidationOutcome::Blocked { .. }
        )
    }
}

pub struct ValidateCommandService {
    state_repo: Box<dyn ArkivSakTilstandRepository>,
    id_mapping: Box<dyn IdMappingRepository>,
    dispatcher: Box<dyn ValidatedCommandDispatcher>,
    status_publisher: Box<dyn CommandStatusPublisher>,
}

impl ValidateCommandService {
    pub fn new(
        state_repo: Box<dyn ArkivSakTilstandRepository>,
        id_mapping: Box<dyn IdMappingRepository>,
        dispatcher: Box<dyn ValidatedCommandDispatcher>,
        status_publisher: Box<dyn CommandStatusPublisher>,
    ) -> Self {
        Self {
            state_repo,
            id_mapping,
            dispatcher,
            status_publisher,
        }
    }

    pub async fn handle(&self, envelope: CommandEnvelope<Command>) -> Result<ValidationOutcome> {
        self.emit_status(&envelope, CommandStatus::Pending, None, None)
            .await?;

        let outcome = match envelope.payload.clone() {
            Command::OpprettSak(_) => ValidationOutcome::Ok,
            Command::OpprettInngåendeJournalpost(c) => {
                self.validate_sak_ref(c.felles.sak_key).await
            }
            Command::OpprettUtgåendeJournalpost(c) => {
                self.validate_sak_ref(c.felles.sak_key).await
            }
            Command::OpprettInterntNotatJournalpost(c) => {
                self.validate_sak_ref(c.felles.sak_key).await
            }
            Command::AvsluttSak(c) => self.validate_sak_ref(c.sak_key).await,
        };

        match outcome {
            ValidationOutcome::Ok => {
                self.dispatcher.dispatch_validated(&envelope).await?;
                self.emit_status(&envelope, CommandStatus::Ok, None, None)
                    .await?;
                Ok(ValidationOutcome::Ok)
            }
            ValidationOutcome::Blocked { message } => {
                self.emit_status(
                    &envelope,
                    CommandStatus::Blocked,
                    Some(message.clone()),
                    None,
                )
                .await?;
                Ok(ValidationOutcome::Blocked { message })
            }
            ValidationOutcome::Recoverable { message } => {
                self.emit_status(
                    &envelope,
                    CommandStatus::Retrying,
                    Some(message.clone()),
                    None,
                )
                .await?;
                Ok(ValidationOutcome::Recoverable { message })
            }
            ValidationOutcome::Irrecoverable { message } => {
                self.emit_status(&envelope, CommandStatus::Error, Some(message.clone()), None)
                    .await?;
                Ok(ValidationOutcome::Irrecoverable { message })
            }
        }
    }

    async fn validate_sak_ref(&self, sak_key: SakKey) -> ValidationOutcome {
        match sak_key {
            SakKey::ClientReference(client_reference) => {
                match self
                    .id_mapping
                    .hent_skuffen_id_fra_mapping(client_reference)
                    .await
                {
                    Ok(Some(skuffen_id)) => {
                        match self.id_mapping.hent_arkiv_id_fra_mapping(skuffen_id).await {
                            Ok(Some(arkiv_id)) => {
                                self.validate_sak_fra_arkivet(arkiv_id.as_str()).await
                            }
                            Ok(None) => ValidationOutcome::Ok,
                            Err(err) => ValidationOutcome::Recoverable {
                                message: err.to_string(),
                            },
                        }
                    }
                    Ok(None) => ValidationOutcome::Irrecoverable {
                        message: "Sak finnes ikke i Skuffen".to_string(),
                    },
                    Err(err) => ValidationOutcome::Recoverable {
                        message: err.to_string(),
                    },
                }
            }
            SakKey::ArkivId(saksnummer) => {
                let outcome = self.validate_sak_fra_arkivet(saksnummer.as_str()).await;
                if matches!(outcome, ValidationOutcome::Irrecoverable { .. }) {
                    let _ = self
                        .id_mapping
                        .delete_arkiv_mapping("sak", saksnummer.as_str())
                        .await;
                }
                outcome
            }
        }
    }

    async fn validate_sak_fra_arkivet(&self, saksnummer: &str) -> ValidationOutcome {
        match self.state_repo.hent_sak_tilstand_fra_arkivet(saksnummer).await {
            Ok(state) => {
                if state.avsluttet {
                    ValidationOutcome::Irrecoverable {
                        message: "Sak er avsluttet".to_string(),
                    }
                } else {
                    ValidationOutcome::Ok
                }
            }
            Err(err) => match err.kind {
                crate::command::ports::command_state_port::ArkivSakTilstandErrorKind::Irrecoverable => {
                    ValidationOutcome::Irrecoverable {
                        message: err.message,
                    }
                }
                crate::command::ports::command_state_port::ArkivSakTilstandErrorKind::Recoverable => {
                    ValidationOutcome::Recoverable {
                        message: err.message,
                    }
                }
            },
        }
    }

    async fn emit_status(
        &self,
        envelope: &CommandEnvelope<Command>,
        status: CommandStatus,
        message: Option<String>,
        attempt: Option<u32>,
    ) -> Result<()> {
        let event = CommandStatusEvent {
            command_id: envelope.command_id,
            status,
            message,
            attempt,
            timestamp: None,
        };
        self.status_publisher.publish_status(event).await
    }
}
