use anyhow::Result;

use crate::command::SakKey;
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
use crate::command::{Command, CommandEnvelope};
use domain::eksekvering::typer::{CommandLifecycleEvent, StatusErrorCode};

pub trait IntoCommandEnvelope {
    fn into_command_envelope(self) -> CommandEnvelope<Command>;
}

impl IntoCommandEnvelope for CommandEnvelope<Command> {
    fn into_command_envelope(self) -> CommandEnvelope<Command> {
        self
    }
}

pub enum ValidationOutcome {
    Ok,
    Blocked {
        message: String,
        error_code: StatusErrorCode,
    },
    Recoverable {
        message: String,
        error_code: StatusErrorCode,
    },
    Irrecoverable {
        message: String,
        error_code: StatusErrorCode,
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
            } => LifecycleDecision::blocked(message.clone(), Some(*error_code)),
            ValidationOutcome::Recoverable {
                message,
                error_code,
            } => LifecycleDecision::retrying(message.clone(), Some(*error_code)),
            ValidationOutcome::Irrecoverable {
                message,
                error_code,
            } => LifecycleDecision::error(message.clone(), Some(*error_code)),
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

    pub async fn handle(&self, envelope: impl IntoCommandEnvelope) -> Result<ValidationOutcome> {
        let envelope = envelope.into_command_envelope();
        let context = self
            .outward_status_projector
            .resolve_context(&envelope)
            .await?;

        let outcome = match envelope.payload.clone() {
            Command::OpprettSak(c) => match validate_sakstittel_markup(&c) {
                ValidationOutcome::Ok => ValidationOutcome::Ok,
                avvist => avvist,
            },
            Command::OpprettInngaaendeJournalpost(c) => match validate_journalpost_lokalt(&c) {
                ValidationOutcome::Ok => self.validate_sak_ref(c.felles().sak_key.clone()).await,
                avvist => avvist,
            },
            Command::OpprettUtgaaendeJournalpost(c) => match validate_journalpost_lokalt(&c) {
                ValidationOutcome::Ok => self.validate_sak_ref(c.felles().sak_key.clone()).await,
                avvist => avvist,
            },
            Command::OpprettInterntNotatJournalpost(c) => match validate_journalpost_lokalt(&c) {
                ValidationOutcome::Ok => self.validate_sak_ref(c.felles().sak_key.clone()).await,
                avvist => avvist,
            },
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
                    decision.error_code,
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
                    decision.error_code,
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
                    decision.error_code,
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
                                error_code: StatusErrorCode::TemporaryUnavailable,
                            },
                        }
                    }
                    Ok(None) => ValidationOutcome::Irrecoverable {
                        message: "Sak finnes ikke i Skuffen".to_string(),
                        error_code: StatusErrorCode::NotFound,
                    },
                    Err(err) => ValidationOutcome::Recoverable {
                        message: err.to_string(),
                        error_code: StatusErrorCode::TemporaryUnavailable,
                    },
                }
            }
            SakKey::ArkivId(saksnummer) => self.validate_sak_fra_arkivet(&saksnummer).await,
        }
    }

    async fn validate_sak_fra_arkivet(&self, saksnummer: &str) -> ValidationOutcome {
        match self.state_repo.hent_sak_tilstand_fra_arkivet(saksnummer).await {
            Ok(state) => {
                if state.avsluttet {
                    ValidationOutcome::Irrecoverable {
                        message: "Sak er avsluttet".to_string(),
                        error_code: StatusErrorCode::Conflict,
                    }
                } else {
                    ValidationOutcome::Ok
                }
            }
            Err(err) => match err.kind {
                crate::command::ports::command_state_port::ArkivSakTilstandErrorKind::Irrecoverable => {
                    ValidationOutcome::Irrecoverable {
                        message: err.message,
                        error_code: StatusErrorCode::InvalidRequest,
                    }
                }
                crate::command::ports::command_state_port::ArkivSakTilstandErrorKind::Recoverable => {
                    ValidationOutcome::Recoverable {
                        message: err.message,
                        error_code: StatusErrorCode::TemporaryUnavailable,
                    }
                }
            },
        }
    }

    async fn emit_status(&self, event: CommandLifecycleEvent) -> Result<()> {
        self.status_publisher.publish_status(event).await
    }
}

/// Lokale (IO-frie) skjermings-invarianter for journalpost-kommandoer:
/// tittel-markup må stemme med journalpostens tilgjengelighet, og
/// korrespondansepart-navn må være markup-frie.
fn validate_journalpost_lokalt(
    command: &crate::command::OpprettJournalpostCommand,
) -> ValidationOutcome {
    use domain::model::skjerming_markup::{
        MarkupSjekk, navn_er_markup_fritt, sjekk_skjerming_markup,
    };

    let felles = command.felles();
    match sjekk_skjerming_markup(&felles.tittel, er_skjermet(&felles.tilgjengelighet)) {
        MarkupSjekk::Ok => {}
        MarkupSjekk::SkjermingKrevesMenMangler => {
            return markup_avvist("Journalposttittel har skjermings-markup uten skjerming");
        }
        MarkupSjekk::UbalansertKlamme => {
            return markup_avvist("Journalposttittel har ubalansert skjermings-markup");
        }
    }

    use crate::command::OpprettJournalpostCommand;
    let navn: Vec<&str> = match command {
        OpprettJournalpostCommand::Inngaende { avsender, .. } => vec![avsender.navn.as_str()],
        OpprettJournalpostCommand::Utgaaende { mottakere, .. } => {
            mottakere.iter().map(|part| part.navn.as_str()).collect()
        }
        OpprettJournalpostCommand::UtgaaendeMedUtsending { mottakere, .. } => mottakere
            .iter()
            .map(|mottaker| mottaker.navn.as_str())
            .collect(),
        OpprettJournalpostCommand::InterntNotat { .. } => Vec::new(),
    };
    let navn_ok = navn.into_iter().all(navn_er_markup_fritt);

    if !navn_ok {
        return markup_avvist("Korrespondansepart-navn skal ikke inneholde markup");
    }

    ValidationOutcome::Ok
}

/// Lokal skjermings-invariant for sakstittel.
fn validate_sakstittel_markup(command: &crate::command::OpprettSakCommand) -> ValidationOutcome {
    use domain::model::skjerming_markup::{MarkupSjekk, sjekk_skjerming_markup};

    match sjekk_skjerming_markup(&command.sakstittel, er_skjermet(&command.tilgjengelighet)) {
        MarkupSjekk::Ok => ValidationOutcome::Ok,
        MarkupSjekk::SkjermingKrevesMenMangler => {
            markup_avvist("Sakstittel har skjermings-markup uten skjerming")
        }
        MarkupSjekk::UbalansertKlamme => {
            markup_avvist("Sakstittel har ubalansert skjermings-markup")
        }
    }
}

fn er_skjermet(tilgjengelighet: &crate::command::Tilgjengelighet) -> bool {
    matches!(
        tilgjengelighet,
        crate::command::Tilgjengelighet::Skjermet { .. }
    )
}

fn markup_avvist(message: &str) -> ValidationOutcome {
    ValidationOutcome::Irrecoverable {
        message: message.to_string(),
        error_code: StatusErrorCode::InvalidRequest,
    }
}
