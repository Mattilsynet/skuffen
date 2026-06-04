use domain::eksekvering::typer::{
    CommandLifecycleContext, CommandLifecycleEvent, CommandLifecycleMetadata, CommandStage,
    CommandStageStatus, CommandStatus, CommandTypeCode, StatusErrorCode,
};

use crate::command::{Command, CommandEnvelope};
pub(crate) fn command_metadata(command: &Command) -> CommandTypeCode {
    match command {
        Command::OpprettSak(_) => CommandTypeCode::OpprettSak,
        Command::OpprettInngaaendeJournalpost(_) => CommandTypeCode::OpprettInngaaendeJournalpost,
        Command::OpprettUtgaaendeJournalpost(_) => CommandTypeCode::OpprettUtgaaendeJournalpost,
        Command::OpprettInterntNotatJournalpost(_) => {
            CommandTypeCode::OpprettInterntNotatJournalpost
        }
        Command::AvsluttSak(_) => CommandTypeCode::AvsluttSak,
        Command::SettSaksansvarlig(_) => CommandTypeCode::SettSaksansvarlig,
    }
}

pub trait StatusEventEnvelope {
    fn command_id(&self) -> uuid::Uuid;
    fn correlation_id(&self) -> Option<uuid::Uuid>;
    fn command_type(&self) -> CommandTypeCode;
}

impl StatusEventEnvelope for CommandEnvelope<Command> {
    fn command_id(&self) -> uuid::Uuid {
        self.command_id
    }

    fn correlation_id(&self) -> Option<uuid::Uuid> {
        self.correlation_id
    }

    fn command_type(&self) -> CommandTypeCode {
        command_metadata(&self.payload)
    }
}

#[allow(clippy::too_many_arguments)]
fn status_event(
    envelope: &(impl StatusEventEnvelope + ?Sized),
    status: CommandStatus,
    stage: CommandStage,
    stage_status: CommandStageStatus,
    error_code: Option<StatusErrorCode>,
    detail: Option<String>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    CommandLifecycleEvent::new(
        CommandLifecycleMetadata::new(envelope.command_id(), envelope.command_type()),
        envelope.correlation_id(),
        status,
        stage,
        stage_status,
        error_code,
        detail,
        context,
        attempt,
    )
}

// Static outward messages for validation non-ok statuses
const VALIDERT_BLOCKED_OUTWARD_MESSAGE: &str = "Request validation is waiting for prerequisites.";
const VALIDERT_RETRYING_OUTWARD_MESSAGE: &str =
    "Request validation is temporarily unavailable and will be retried.";
const VALIDERT_ERROR_OUTWARD_MESSAGE: &str = "Request validation failed.";

fn outward_message(stage: CommandStage, stage_status: CommandStageStatus) -> &'static str {
    match (stage, stage_status) {
        (CommandStage::Mottatt, CommandStageStatus::Ok) => "Request accepted for processing.",
        (CommandStage::Validert, CommandStageStatus::Ok) => "Request validated successfully.",
        (CommandStage::Validert, CommandStageStatus::Blocked) => VALIDERT_BLOCKED_OUTWARD_MESSAGE,
        (CommandStage::Validert, CommandStageStatus::Retrying) => VALIDERT_RETRYING_OUTWARD_MESSAGE,
        (CommandStage::Validert, CommandStageStatus::Error) => VALIDERT_ERROR_OUTWARD_MESSAGE,
        (CommandStage::Utfores, CommandStageStatus::Venter) => "Command is queued for execution.",
        (CommandStage::Utfores, CommandStageStatus::Retrying) => {
            "Command execution is temporarily unavailable and will be retried."
        }
        (CommandStage::Utfores, CommandStageStatus::Blocked) => {
            "Command execution is waiting for prerequisites."
        }
        (CommandStage::Utfores, CommandStageStatus::Error) => "Command execution failed.",
        _ => unreachable!("outward_message only supports lifecycle states without explicit detail"),
    }
}

pub fn mottatt_event(
    envelope: &(impl StatusEventEnvelope + ?Sized),
    context: CommandLifecycleContext,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Pending,
        CommandStage::Mottatt,
        CommandStageStatus::Ok,
        None,
        None,
        context,
        None,
    )
    .with_outward_message(outward_message(
        CommandStage::Mottatt,
        CommandStageStatus::Ok,
    ))
}

pub fn validert_ok_event(
    envelope: &(impl StatusEventEnvelope + ?Sized),
    context: CommandLifecycleContext,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Ok,
        CommandStage::Validert,
        CommandStageStatus::Ok,
        None,
        None,
        context,
        None,
    )
    .with_outward_message(outward_message(
        CommandStage::Validert,
        CommandStageStatus::Ok,
    ))
}

pub fn validert_blocked_event(
    envelope: &(impl StatusEventEnvelope + ?Sized),
    detail: impl Into<String>,
    error_code: Option<StatusErrorCode>,
    context: CommandLifecycleContext,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Blocked,
        CommandStage::Validert,
        CommandStageStatus::Blocked,
        error_code.or(Some(StatusErrorCode::PrerequisitePending)),
        Some(detail.into()),
        context,
        None,
    )
    .with_outward_message(VALIDERT_BLOCKED_OUTWARD_MESSAGE)
}

pub fn validert_retrying_event(
    envelope: &(impl StatusEventEnvelope + ?Sized),
    detail: impl Into<String>,
    error_code: Option<StatusErrorCode>,
    context: CommandLifecycleContext,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Retrying,
        CommandStage::Validert,
        CommandStageStatus::Retrying,
        error_code.or(Some(StatusErrorCode::TemporaryUnavailable)),
        Some(detail.into()),
        context,
        None,
    )
    .with_outward_message(VALIDERT_RETRYING_OUTWARD_MESSAGE)
}

pub fn validert_error_event(
    envelope: &(impl StatusEventEnvelope + ?Sized),
    detail: impl Into<String>,
    error_code: Option<StatusErrorCode>,
    context: CommandLifecycleContext,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Error,
        CommandStage::Validert,
        CommandStageStatus::Error,
        error_code.or(Some(StatusErrorCode::InvalidRequest)),
        Some(detail.into()),
        context,
        None,
    )
    .with_outward_message(VALIDERT_ERROR_OUTWARD_MESSAGE)
}

pub fn utfores_venter_event(
    envelope: &(impl StatusEventEnvelope + ?Sized),
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Pending,
        CommandStage::Utfores,
        CommandStageStatus::Venter,
        None,
        None,
        context,
        attempt,
    )
    .with_outward_message(outward_message(
        CommandStage::Utfores,
        CommandStageStatus::Venter,
    ))
}

pub fn utfores_ok_event(
    envelope: &(impl StatusEventEnvelope + ?Sized),
    detail: Option<String>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Ok,
        CommandStage::Utfores,
        CommandStageStatus::Ok,
        None,
        detail,
        context,
        attempt,
    )
    .with_outward_message("Command executed successfully.")
}

pub fn utfores_retrying_event(
    envelope: &(impl StatusEventEnvelope + ?Sized),
    detail: impl Into<String>,
    error_code: Option<StatusErrorCode>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    let detail = detail.into();
    status_event(
        envelope,
        CommandStatus::Retrying,
        CommandStage::Utfores,
        CommandStageStatus::Retrying,
        error_code.or(Some(StatusErrorCode::TemporaryUnavailable)),
        Some(detail.clone()),
        context,
        attempt,
    )
    .with_outward_message(outward_message(
        CommandStage::Utfores,
        CommandStageStatus::Retrying,
    ))
}

pub fn utfores_blocked_event(
    envelope: &(impl StatusEventEnvelope + ?Sized),
    detail: impl Into<String>,
    error_code: Option<StatusErrorCode>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    let detail = detail.into();
    status_event(
        envelope,
        CommandStatus::Blocked,
        CommandStage::Utfores,
        CommandStageStatus::Blocked,
        error_code.or(Some(StatusErrorCode::PrerequisitePending)),
        Some(detail.clone()),
        context,
        attempt,
    )
    .with_outward_message(outward_message(
        CommandStage::Utfores,
        CommandStageStatus::Blocked,
    ))
}

pub fn utfores_error_event(
    envelope: &(impl StatusEventEnvelope + ?Sized),
    detail: impl Into<String>,
    error_code: Option<StatusErrorCode>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    let detail = detail.into();
    status_event(
        envelope,
        CommandStatus::Error,
        CommandStage::Utfores,
        CommandStageStatus::Error,
        error_code.or(Some(StatusErrorCode::ProcessingFailed)),
        Some(detail.clone()),
        context,
        attempt,
    )
    .with_outward_message(outward_message(
        CommandStage::Utfores,
        CommandStageStatus::Error,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Arkivdel, OpprettSakCommand};
    use uuid::Uuid;

    fn make_test_envelope() -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id: Uuid::new_v4(),
            correlation_id: Some(Uuid::new_v4()),
            payload: Command::OpprettSak(OpprettSakCommand {
                client_reference: Uuid::new_v4(),
                sakstittel: "Test sak".to_string(),
                arkivdel: Arkivdel::Tilsynsdivisjonene,
                saksbehandler_id: "Z12345".to_string(),
                saksbehandler_enhet: "42".to_string(),
                ordningsverdi: "123".to_string(),
                tilgang: None,
            }),
        }
    }

    #[test]
    fn validert_blocked_event_retains_detail_with_static_outward_message() {
        let envelope = make_test_envelope();
        let dynamic_detail = "Internal prerequisite details here".to_string();
        let event = validert_blocked_event(
            &envelope,
            dynamic_detail.clone(),
            None,
            CommandLifecycleContext::default(),
        );

        assert_eq!(event.detail, Some(dynamic_detail));
        assert_eq!(
            event.outward_message,
            Some(VALIDERT_BLOCKED_OUTWARD_MESSAGE.to_string())
        );
        assert_ne!(event.detail, event.outward_message);
    }

    #[test]
    fn validert_retrying_event_retains_detail_with_static_outward_message() {
        let envelope = make_test_envelope();
        let dynamic_detail = "Validation service temporarily down".to_string();
        let event = validert_retrying_event(
            &envelope,
            dynamic_detail.clone(),
            None,
            CommandLifecycleContext::default(),
        );

        assert_eq!(event.detail, Some(dynamic_detail));
        assert_eq!(
            event.outward_message,
            Some(VALIDERT_RETRYING_OUTWARD_MESSAGE.to_string())
        );
        assert_ne!(event.detail, event.outward_message);
    }

    #[test]
    fn validert_error_event_retains_detail_with_static_outward_message() {
        let envelope = make_test_envelope();
        let dynamic_detail = "Invalid field: email format".to_string();
        let event = validert_error_event(
            &envelope,
            dynamic_detail.clone(),
            None,
            CommandLifecycleContext::default(),
        );

        assert_eq!(event.detail, Some(dynamic_detail));
        assert_eq!(
            event.outward_message,
            Some(VALIDERT_ERROR_OUTWARD_MESSAGE.to_string())
        );
        assert_ne!(event.detail, event.outward_message);
    }

    #[test]
    fn utfores_ok_event_keeps_detail_internal_with_static_outward_message() {
        let envelope = make_test_envelope();
        let dynamic_detail = Some("Detailed execution result".to_string());
        let event = utfores_ok_event(
            &envelope,
            dynamic_detail.clone(),
            CommandLifecycleContext::default(),
            None,
        );

        assert_eq!(event.detail, dynamic_detail);
        assert_eq!(
            event.outward_message,
            Some("Command executed successfully.".to_string())
        );
        assert_ne!(event.detail, event.outward_message);
    }
}
