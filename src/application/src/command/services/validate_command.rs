use anyhow::Result;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::query::queries::SakKey;
use lib_schemas::skuffen::status::SkuffenStatusErrorCode;

use crate::command::lifecycle::LifecycleDecision;
use crate::command::ports::{
    command_state_port::ArkivSakTilstandRepository, id_mapping_port::IdMappingRepository,
    status_projection_port::CommandOutwardStatusProjector,
    status_publisher_port::CommandStatusPublisher,
    validated_command_dispatcher_port::ValidatedCommandDispatcher,
};
use crate::command::status::{
    validert_blocked_event, validert_error_event, validert_ok_event, validert_retrying_event,
};
use domain::eksekvering::typer::CommandLifecycleEvent;

pub enum ValidationOutcome {
    Ok,
    Blocked {
        message: String,
        error_code: SkuffenStatusErrorCode,
    },
    Recoverable {
        message: String,
        error_code: SkuffenStatusErrorCode,
    },
    Irrecoverable {
        message: String,
        error_code: SkuffenStatusErrorCode,
    },
}

impl ValidationOutcome {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ValidationOutcome::Recoverable { .. } | ValidationOutcome::Blocked { .. }
        )
    }

    fn as_lifecycle_decision(&self) -> LifecycleDecision {
        match self {
            ValidationOutcome::Ok => LifecycleDecision::ok(None),
            ValidationOutcome::Blocked {
                message,
                error_code,
            } => LifecycleDecision::blocked(message.clone(), Some(error_code.clone())),
            ValidationOutcome::Recoverable {
                message,
                error_code,
            } => LifecycleDecision::retrying(message.clone(), Some(error_code.clone())),
            ValidationOutcome::Irrecoverable {
                message,
                error_code,
            } => LifecycleDecision::error(message.clone(), Some(error_code.clone())),
        }
    }
}

/// Validerer innkommende kommandoer uten a materialisere state i eksekveringssystemet.
///
/// Ansvar:
/// - referansegyldighet
/// - logisk gyldighet
/// - Arkiv-oppslag for sak nar `SakKey::ArkivId` eller kjent `arkiv_id` finnes
///
/// Ikke ansvar:
/// - `sak_state`
/// - `journalpost_state`
/// - `dokument_state`
/// - `command_execution`
pub struct ValidateCommandService {
    state_repo: Box<dyn ArkivSakTilstandRepository>,
    id_mapping: Box<dyn IdMappingRepository>,
    dispatcher: Box<dyn ValidatedCommandDispatcher>,
    status_publisher: Box<dyn CommandStatusPublisher>,
    outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
}

impl ValidateCommandService {
    /// Validering eier referanse- og regelkontroller for innkommende kommandoer.
    ///
    /// Denne fasen kan bruke Arkiv-oppslag og `id_mapping`, men skal ikke
    /// materialisere lokalt eksekverings-state eller skrive til
    /// `command_execution`.
    pub fn new(
        state_repo: Box<dyn ArkivSakTilstandRepository>,
        id_mapping: Box<dyn IdMappingRepository>,
        dispatcher: Box<dyn ValidatedCommandDispatcher>,
        status_publisher: Box<dyn CommandStatusPublisher>,
        outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
    ) -> Self {
        Self {
            state_repo,
            id_mapping,
            dispatcher,
            status_publisher,
            outward_status_projector,
        }
    }

    pub async fn handle(&self, envelope: CommandEnvelope<Command>) -> Result<ValidationOutcome> {
        let context = self
            .outward_status_projector
            .resolve_context(&envelope)
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
            Command::SettSaksansvarlig(c) => self.validate_sak_ref(c.sak_key).await,
        };
        let decision = outcome.as_lifecycle_decision();

        match outcome {
            ValidationOutcome::Ok => {
                self.dispatcher.dispatch_validated(&envelope).await?;
                self.emit_status(validert_ok_event(&envelope, context))
                    .await?;
                Ok(ValidationOutcome::Ok)
            }
            ValidationOutcome::Blocked {
                message,
                error_code,
            } => {
                self.emit_status(validert_blocked_event(
                    &envelope,
                    decision.detail.clone().unwrap_or_else(|| message.clone()),
                    decision.error_code.clone(),
                    context.clone(),
                ))
                .await?;
                Ok(ValidationOutcome::Blocked {
                    message,
                    error_code,
                })
            }
            ValidationOutcome::Recoverable {
                message,
                error_code,
            } => {
                self.emit_status(validert_retrying_event(
                    &envelope,
                    decision.detail.clone().unwrap_or_else(|| message.clone()),
                    decision.error_code.clone(),
                    context.clone(),
                ))
                .await?;
                Ok(ValidationOutcome::Recoverable {
                    message,
                    error_code,
                })
            }
            ValidationOutcome::Irrecoverable {
                message,
                error_code,
            } => {
                self.emit_status(validert_error_event(
                    &envelope,
                    decision.detail.clone().unwrap_or_else(|| message.clone()),
                    decision.error_code.clone(),
                    context,
                ))
                .await?;
                Ok(ValidationOutcome::Irrecoverable {
                    message,
                    error_code,
                })
            }
        }
    }

    async fn validate_sak_ref(&self, sak_key: SakKey) -> ValidationOutcome {
        match sak_key {
            SakKey::ClientReference(client_reference) => {
                match self
                    .id_mapping
                    .hent_sak_id_fra_mapping(client_reference)
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
                                error_code: SkuffenStatusErrorCode::TemporaryUnavailable,
                            },
                        }
                    }
                    Ok(None) => ValidationOutcome::Irrecoverable {
                        message: "Sak finnes ikke i Skuffen".to_string(),
                        error_code: SkuffenStatusErrorCode::NotFound,
                    },
                    Err(err) => ValidationOutcome::Recoverable {
                        message: err.to_string(),
                        error_code: SkuffenStatusErrorCode::TemporaryUnavailable,
                    },
                }
            }
            SakKey::ArkivId(saksnummer) => self.validate_sak_fra_arkivet(saksnummer.as_str()).await,
        }
    }

    async fn validate_sak_fra_arkivet(&self, saksnummer: &str) -> ValidationOutcome {
        match self.state_repo.hent_sak_tilstand_fra_arkivet(saksnummer).await {
            Ok(state) => {
                if state.avsluttet {
                    ValidationOutcome::Irrecoverable {
                        message: "Sak er avsluttet".to_string(),
                        error_code: SkuffenStatusErrorCode::Conflict,
                    }
                } else {
                    ValidationOutcome::Ok
                }
            }
            Err(err) => match err.kind {
                crate::command::ports::command_state_port::ArkivSakTilstandErrorKind::Irrecoverable => {
                    ValidationOutcome::Irrecoverable {
                        message: err.message,
                        error_code: SkuffenStatusErrorCode::InvalidRequest,
                    }
                }
                crate::command::ports::command_state_port::ArkivSakTilstandErrorKind::Recoverable => {
                    ValidationOutcome::Recoverable {
                        message: err.message,
                        error_code: SkuffenStatusErrorCode::TemporaryUnavailable,
                    }
                }
            },
        }
    }

    async fn emit_status(&self, event: CommandLifecycleEvent) -> Result<()> {
        self.status_publisher.publish_status(event).await
    }
}
