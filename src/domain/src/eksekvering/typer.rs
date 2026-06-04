use chrono::Utc;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Pending,
    Ok,
    Blocked,
    Retrying,
    Error,
}

impl CommandStatus {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ok => "ok",
            Self::Blocked => "blocked",
            Self::Retrying => "retrying",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusErrorCode {
    InvalidRequest,
    NotFound,
    Conflict,
    PrerequisitePending,
    TemporaryUnavailable,
    ProcessingFailed,
}

impl StatusErrorCode {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::PrerequisitePending => "prerequisite_pending",
            Self::TemporaryUnavailable => "temporary_unavailable",
            Self::ProcessingFailed => "processing_failed",
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
    SettSaksansvarlig,
}

impl CommandTypeCode {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::OpprettSak => "opprett_sak",
            Self::OpprettInngaaendeJournalpost => "opprett_inngaaende_journalpost",
            Self::OpprettUtgaaendeJournalpost => "opprett_utgaaende_journalpost",
            Self::OpprettInterntNotatJournalpost => "opprett_internt_notat_journalpost",
            Self::AvsluttSak => "avslutt_sak",
            Self::SettSaksansvarlig => "sett_saksansvarlig",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandLifecycleMetadata {
    pub command_id: Uuid,
    pub command_type: CommandTypeCode,
}

impl CommandLifecycleMetadata {
    pub fn new(command_id: Uuid, command_type: CommandTypeCode) -> Self {
        Self {
            command_id,
            command_type,
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
    pub correlation_id: Option<Uuid>,
    pub command_type: CommandTypeCode,
    pub status: CommandStatus,
    pub stage: CommandStage,
    pub stage_status: CommandStageStatus,
    pub terminal: bool,
    pub message: String,
    pub outward_message: Option<String>,
    pub error_code: Option<StatusErrorCode>,
    pub detail: Option<String>,
    pub context: CommandLifecycleContext,
    pub attempt: Option<u32>,
    pub timestamp: Option<String>,
}

impl CommandLifecycleEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        metadata: CommandLifecycleMetadata,
        correlation_id: Option<Uuid>,
        status: CommandStatus,
        stage: CommandStage,
        stage_status: CommandStageStatus,
        error_code: Option<StatusErrorCode>,
        detail: Option<String>,
        context: CommandLifecycleContext,
        attempt: Option<u32>,
    ) -> Self {
        Self {
            command_id: metadata.command_id,
            correlation_id,
            command_type: metadata.command_type,
            status,
            stage,
            stage_status,
            terminal: is_terminal(stage, stage_status),
            message: status_message(stage, stage_status),
            outward_message: None,
            error_code,
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

    pub fn with_outward_message(mut self, outward_message: impl Into<String>) -> Self {
        self.outward_message = Some(outward_message.into());
        self
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mottatt_uses_stage_name_as_message() {
        let event = CommandLifecycleEvent::new(
            CommandLifecycleMetadata::new(Uuid::new_v4(), CommandTypeCode::OpprettSak),
            None,
            CommandStatus::Pending,
            CommandStage::Mottatt,
            CommandStageStatus::Ok,
            None,
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
            CommandLifecycleMetadata::new(command_id, CommandTypeCode::OpprettSak),
            None,
            CommandStatus::Retrying,
            CommandStage::Utfores,
            CommandStageStatus::Retrying,
            None,
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

    #[test]
    fn command_status_literals_are_pinned() {
        let values = [
            (CommandStatus::Pending, "pending"),
            (CommandStatus::Ok, "ok"),
            (CommandStatus::Blocked, "blocked"),
            (CommandStatus::Retrying, "retrying"),
            (CommandStatus::Error, "error"),
        ];

        for (status, code) in values {
            assert_eq!(status.as_code(), code);
        }
    }

    #[test]
    fn status_error_code_literals_are_pinned() {
        let values = [
            (StatusErrorCode::InvalidRequest, "invalid_request"),
            (StatusErrorCode::NotFound, "not_found"),
            (StatusErrorCode::Conflict, "conflict"),
            (StatusErrorCode::PrerequisitePending, "prerequisite_pending"),
            (
                StatusErrorCode::TemporaryUnavailable,
                "temporary_unavailable",
            ),
            (StatusErrorCode::ProcessingFailed, "processing_failed"),
        ];

        for (error_code, code) in values {
            assert_eq!(error_code.as_code(), code);
        }
    }

    #[test]
    fn command_type_code_literals_are_pinned_for_persistence_and_routing() {
        let values = [
            (CommandTypeCode::OpprettSak, "opprett_sak"),
            (
                CommandTypeCode::OpprettInngaaendeJournalpost,
                "opprett_inngaaende_journalpost",
            ),
            (
                CommandTypeCode::OpprettUtgaaendeJournalpost,
                "opprett_utgaaende_journalpost",
            ),
            (
                CommandTypeCode::OpprettInterntNotatJournalpost,
                "opprett_internt_notat_journalpost",
            ),
            (CommandTypeCode::AvsluttSak, "avslutt_sak"),
            (CommandTypeCode::SettSaksansvarlig, "sett_saksansvarlig"),
        ];

        for (command_type, code) in values {
            assert_eq!(command_type.as_code(), code);
        }
    }

    #[test]
    fn lifecycle_context_is_empty_only_without_identifiers() {
        let empty = CommandLifecycleContext::default();
        assert!(empty.is_empty());

        let with_sak = CommandLifecycleContext {
            sak_client_reference: Some(Uuid::new_v4().to_string()),
            ..CommandLifecycleContext::default()
        };
        assert!(!with_sak.is_empty());
    }
}
