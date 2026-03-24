use chrono::Utc;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope, CommandStatus};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandEntityType {
    Sak,
    Journalpost,
}

impl CommandEntityType {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Sak => "sak",
            Self::Journalpost => "journalpost",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTypeCode {
    OpprettSak,
    OpprettInngaaendeJournalpost,
    OpprettUtgaaendeJournalpost,
    OpprettInterntNotatJournalpost,
    AvsluttSak,
}

impl CommandTypeCode {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::OpprettSak => "opprett_sak",
            Self::OpprettInngaaendeJournalpost => "opprett_inngaaende_journalpost",
            Self::OpprettUtgaaendeJournalpost => "opprett_utgaaende_journalpost",
            Self::OpprettInterntNotatJournalpost => "opprett_internt_notat_journalpost",
            Self::AvsluttSak => "avslutt_sak",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandLifecycleMetadata {
    pub command_id: Uuid,
    pub command_type: CommandTypeCode,
    pub entity_type: CommandEntityType,
}

impl CommandLifecycleMetadata {
    pub fn new(
        command_id: Uuid,
        command_type: CommandTypeCode,
        entity_type: CommandEntityType,
    ) -> Self {
        Self {
            command_id,
            command_type,
            entity_type,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStage {
    Mottatt,
    Validert,
    Utfores,
}

impl CommandStage {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Mottatt => "mottatt",
            Self::Validert => "validert",
            Self::Utfores => "utfores",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStageStatus {
    Venter,
    Ok,
    Blocked,
    Retrying,
    Error,
}

impl CommandStageStatus {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Venter => "venter",
            Self::Ok => "ok",
            Self::Blocked => "blocked",
            Self::Retrying => "retrying",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandLifecycleContext {
    pub sak_client_reference: Option<String>,
    pub saksnummer: Option<String>,
    pub journalpost_client_reference: Option<String>,
    pub journalpost_id: Option<String>,
    pub dokument_client_references: Vec<String>,
    pub dokument_ids: Vec<String>,
}

impl CommandLifecycleContext {
    pub fn is_empty(&self) -> bool {
        self.sak_client_reference.is_none()
            && self.saksnummer.is_none()
            && self.journalpost_client_reference.is_none()
            && self.journalpost_id.is_none()
            && self.dokument_client_references.is_empty()
            && self.dokument_ids.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLifecycleEvent {
    pub command_id: Uuid,
    pub command_type: CommandTypeCode,
    pub entity_type: CommandEntityType,
    pub status: CommandStatus,
    pub stage: CommandStage,
    pub stage_status: CommandStageStatus,
    pub terminal: bool,
    pub message: String,
    pub detail: Option<String>,
    pub context: CommandLifecycleContext,
    pub attempt: Option<u32>,
    pub timestamp: Option<String>,
}

impl CommandLifecycleEvent {
    pub fn new(
        metadata: CommandLifecycleMetadata,
        status: CommandStatus,
        stage: CommandStage,
        stage_status: CommandStageStatus,
        detail: Option<String>,
        context: CommandLifecycleContext,
        attempt: Option<u32>,
    ) -> Self {
        Self {
            command_id: metadata.command_id,
            command_type: metadata.command_type,
            entity_type: metadata.entity_type,
            status,
            stage,
            stage_status,
            terminal: is_terminal(stage, stage_status),
            message: status_message(stage, stage_status),
            detail,
            context,
            attempt,
            timestamp: Some(Utc::now().to_rfc3339()),
        }
    }

    pub fn message_id(&self) -> String {
        match self.attempt {
            Some(attempt) => format!(
                "status:{}:{}:{}:{}",
                self.command_id,
                self.stage.as_code(),
                self.stage_status.as_code(),
                attempt
            ),
            None => format!(
                "status:{}:{}:{}",
                self.command_id,
                self.stage.as_code(),
                self.stage_status.as_code()
            ),
        }
    }
}

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

pub fn status_message(stage: CommandStage, stage_status: CommandStageStatus) -> String {
    if matches!(
        (stage, stage_status),
        (CommandStage::Mottatt, CommandStageStatus::Ok)
    ) {
        stage.as_code().to_string()
    } else {
        format!("{}::{}", stage.as_code(), stage_status.as_code())
    }
}

pub fn is_terminal(stage: CommandStage, stage_status: CommandStageStatus) -> bool {
    match (stage, stage_status) {
        (CommandStage::Mottatt, _) => false,
        (CommandStage::Validert, CommandStageStatus::Ok) => false,
        (CommandStage::Validert, _) => true,
        (CommandStage::Utfores, CommandStageStatus::Venter) => false,
        (CommandStage::Utfores, CommandStageStatus::Retrying) => false,
        (CommandStage::Utfores, CommandStageStatus::Blocked) => false,
        (CommandStage::Utfores, CommandStageStatus::Ok) => true,
        (CommandStage::Utfores, CommandStageStatus::Error) => true,
    }
}

pub fn status_event(
    envelope: &CommandEnvelope<Command>,
    status: CommandStatus,
    stage: CommandStage,
    stage_status: CommandStageStatus,
    detail: Option<String>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    let (command_type, entity_type) = command_metadata(&envelope.payload);

    CommandLifecycleEvent::new(
        CommandLifecycleMetadata::new(envelope.command_id, command_type, entity_type),
        status,
        stage,
        stage_status,
        detail,
        context,
        attempt,
    )
}

pub fn command_metadata(command: &Command) -> (CommandTypeCode, CommandEntityType) {
    match command {
        Command::OpprettSak(_) => (CommandTypeCode::OpprettSak, CommandEntityType::Sak),
        Command::OpprettInngåendeJournalpost(_) => (
            CommandTypeCode::OpprettInngaaendeJournalpost,
            CommandEntityType::Journalpost,
        ),
        Command::OpprettUtgåendeJournalpost(_) => (
            CommandTypeCode::OpprettUtgaaendeJournalpost,
            CommandEntityType::Journalpost,
        ),
        Command::OpprettInterntNotatJournalpost(_) => (
            CommandTypeCode::OpprettInterntNotatJournalpost,
            CommandEntityType::Journalpost,
        ),
        Command::AvsluttSak(_) => (CommandTypeCode::AvsluttSak, CommandEntityType::Sak),
    }
}

pub fn done_subject(command: &CommandEnvelope<Command>) -> (String, Uuid) {
    let (_, entity_type) = command_metadata(&command.payload);
    (
        format!(
            "arkiv.command.done.{}.{}",
            entity_type.as_code(),
            command.command_id
        ),
        command.command_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mottatt_uses_stage_name_as_message() {
        let event = CommandLifecycleEvent::new(
            CommandLifecycleMetadata::new(
                Uuid::new_v4(),
                CommandTypeCode::OpprettSak,
                CommandEntityType::Sak,
            ),
            CommandStatus::Pending,
            CommandStage::Mottatt,
            CommandStageStatus::Ok,
            None,
            CommandLifecycleContext::default(),
            None,
        );

        assert_eq!(event.message, "mottatt");
    }

    #[test]
    fn retrying_message_id_includes_attempt() {
        let command_id = Uuid::new_v4();
        let event = CommandLifecycleEvent::new(
            CommandLifecycleMetadata::new(
                command_id,
                CommandTypeCode::OpprettSak,
                CommandEntityType::Sak,
            ),
            CommandStatus::Retrying,
            CommandStage::Utfores,
            CommandStageStatus::Retrying,
            Some("Sikri timeout".to_string()),
            CommandLifecycleContext::default(),
            Some(2),
        );

        assert_eq!(event.message, "utfores::retrying");
        assert_eq!(
            event.message_id(),
            format!("status:{command_id}:utfores:retrying:2")
        );
    }
}
