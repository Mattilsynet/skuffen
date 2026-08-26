use anyhow::Result;

use crate::command::SakKey;
use crate::command::ports::{
    command_state_port::ArkivSakTilstandRepository, entitet_port::EntitetRepository,
    status_publisher_port::StatusPublisher,
    validated_command_dispatcher_port::ValidatedCommandDispatcher,
};
use crate::command::services::ingest_command::{command_type, kontekst};
use crate::command::{Command, CommandEnvelope};
use domain::eksekvering::typer::{CommandEvent, CommandStatus, StatusErrorCode};

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
    entitet: Box<dyn EntitetRepository>,
    dispatcher: Box<dyn ValidatedCommandDispatcher>,
    status_publisher: Box<dyn StatusPublisher>,
}

impl ValidateCommandService {
    /// Validering eier referanse- og regelkontroller for innkommende kommandoer.
    ///
    /// Denne fasen kan bruke Arkiv-oppslag og `entitet`, men skal ikke
    /// materialisere lokalt eksekverings-state eller skrive til
    /// `command_execution`.
    pub fn new(
        state_repo: Box<dyn ArkivSakTilstandRepository>,
        entitet: Box<dyn EntitetRepository>,
        dispatcher: Box<dyn ValidatedCommandDispatcher>,
        status_publisher: Box<dyn StatusPublisher>,
    ) -> Self {
        Self {
            state_repo,
            entitet,
            dispatcher,
            status_publisher,
        }
    }

    pub async fn handle(&self, envelope: impl IntoCommandEnvelope) -> Result<ValidationOutcome> {
        let envelope = envelope.into_command_envelope();

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
        match outcome {
            ValidationOutcome::Ok => {
                self.dispatcher.dispatch_validated(&envelope).await?;
                self.emit(
                    &envelope,
                    CommandEvent::Validert,
                    "Forespørselen er validert.",
                    None,
                )
                .await?;
                Ok(ValidationOutcome::Ok)
            }
            ValidationOutcome::Irrecoverable {
                message,
                error_code,
            } => {
                // Klienten får den faktiske grunnen, ikke «Forespørselen ble
                // avvist.». Meldingene er allerede sanitiserte: de sier hva
                // som er galt uten å gjenta innholdet som var galt.
                self.emit(&envelope, CommandEvent::Avvist, &message, Some(error_code))
                    .await?;
                Ok(ValidationOutcome::Irrecoverable {
                    message,
                    error_code,
                })
            }
            // Blokkert og recoverable er transiente: kommandoen redeliveres av
            // NATS. Vi publiserer ikke flakking, bare utfall (D33).
            other => Ok(other),
        }
    }

    async fn emit(
        &self,
        envelope: &CommandEnvelope<Command>,
        hendelse: CommandEvent,
        melding: &str,
        error_code: Option<StatusErrorCode>,
    ) -> Result<()> {
        self.status_publisher
            .publiser_command_status(CommandStatus::new(
                envelope.command_id,
                envelope.correlation_id,
                command_type(&envelope.payload),
                hendelse,
                melding,
                error_code,
                kontekst(&envelope.payload),
            ))
            .await
    }

    async fn validate_sak_ref(&self, sak_key: SakKey) -> ValidationOutcome {
        match sak_key {
            SakKey::ClientReference(client_reference) => {
                match self
                    .entitet
                    .hent_for_client_reference(client_reference)
                    .await
                {
                    Ok(Some(entitet)) => match entitet.arkiv_id {
                        Some(arkiv_id) => self.validate_sak_fra_arkivet(arkiv_id.as_str()).await,
                        None => ValidationOutcome::Ok,
                    },
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
            // Melding og error_code kommer fra adapteren, som er der
            // klassifiseringen faktisk finnes. Et ukjent saksnummer skal gi
            // NotFound, ikke InvalidRequest for alt.
            Err(err) => match err.kind {
                crate::command::ports::command_state_port::ArkivSakTilstandErrorKind::Irrecoverable => {
                    ValidationOutcome::Irrecoverable {
                        message: err.message,
                        error_code: err.error_code,
                    }
                }
                crate::command::ports::command_state_port::ArkivSakTilstandErrorKind::Recoverable => {
                    ValidationOutcome::Recoverable {
                        message: err.message,
                        error_code: err.error_code,
                    }
                }
            },
        }
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
