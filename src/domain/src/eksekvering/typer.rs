use lib_schemas::skuffen::command::commands::{
    Command, CommandEnvelope, CommandStatus, CommandStatusEvent,
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EksekveringFeiltype {
    Recoverable,
    Irrecoverable,
    Blocked,
}

#[derive(Debug, Clone)]
pub struct EksekveringFeil {
    pub feiltype: EksekveringFeiltype,
    pub melding: String,
}

impl EksekveringFeil {
    pub fn recoverable(melding: impl Into<String>) -> Self {
        Self {
            feiltype: EksekveringFeiltype::Recoverable,
            melding: melding.into(),
        }
    }

    pub fn irrecoverable(melding: impl Into<String>) -> Self {
        Self {
            feiltype: EksekveringFeiltype::Irrecoverable,
            melding: melding.into(),
        }
    }

    pub fn blocked(melding: impl Into<String>) -> Self {
        Self {
            feiltype: EksekveringFeiltype::Blocked,
            melding: melding.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EksekveringResultat {
    Ok,
    Blocked,
    Retrying,
    Error,
}

pub fn status_event(
    envelope: &CommandEnvelope<Command>,
    status: CommandStatus,
    message: Option<String>,
    attempt: Option<u32>,
) -> CommandStatusEvent {
    CommandStatusEvent {
        command_id: envelope.command_id,
        status,
        message,
        attempt,
        timestamp: None,
    }
}

pub fn done_subject(command: &CommandEnvelope<Command>) -> (String, Uuid) {
    let entity_type = match &command.payload {
        Command::OpprettSak(_) | Command::AvsluttSak(_) => "sak",
        Command::OpprettInngåendeJournalpost(_)
        | Command::OpprettUtgåendeJournalpost(_)
        | Command::OpprettInterntNotatJournalpost(_) => "journalpost",
    };
    (
        format!("arkiv.command.done.{}.{}", entity_type, command.command_id),
        command.command_id,
    )
}
