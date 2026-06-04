use domain::eksekvering::typer::{
    CommandLifecycleEvent, CommandStage, CommandStageStatus, StatusErrorCode,
};
use lib_schemas::skuffen::journalpost::JournalpostId;
use lib_schemas::skuffen::sak::Saksnummer;
use lib_schemas::skuffen::status::{
    SkuffenStatus, SkuffenStatusErrorCode, SkuffenStatusEventV1, SkuffenStatusPhase,
};

fn phase_for(stage: CommandStage) -> SkuffenStatusPhase {
    match stage {
        CommandStage::Mottatt => SkuffenStatusPhase::Ingest,
        CommandStage::Validert => SkuffenStatusPhase::Validate,
        CommandStage::Utfores => SkuffenStatusPhase::Execution,
    }
}

fn status_for(stage_status: CommandStageStatus) -> SkuffenStatus {
    match stage_status {
        CommandStageStatus::Venter => SkuffenStatus::Pending,
        CommandStageStatus::Ok => SkuffenStatus::Ok,
        CommandStageStatus::Blocked => SkuffenStatus::Blocked,
        CommandStageStatus::Retrying => SkuffenStatus::Retrying,
        CommandStageStatus::Error => SkuffenStatus::Error,
    }
}

fn error_code_for(error_code: StatusErrorCode) -> SkuffenStatusErrorCode {
    match error_code {
        StatusErrorCode::InvalidRequest => SkuffenStatusErrorCode::InvalidRequest,
        StatusErrorCode::NotFound => SkuffenStatusErrorCode::NotFound,
        StatusErrorCode::Conflict => SkuffenStatusErrorCode::Conflict,
        StatusErrorCode::PrerequisitePending => SkuffenStatusErrorCode::PrerequisitePending,
        StatusErrorCode::TemporaryUnavailable => SkuffenStatusErrorCode::TemporaryUnavailable,
        StatusErrorCode::ProcessingFailed => SkuffenStatusErrorCode::ProcessingFailed,
    }
}

pub fn to_public_status_event(event: &CommandLifecycleEvent) -> SkuffenStatusEventV1 {
    SkuffenStatusEventV1 {
        command_id: event.command_id,
        correlation_id: event.correlation_id,
        phase: phase_for(event.stage),
        status: status_for(event.stage_status),
        terminal: event.terminal,
        error_code: event.error_code.map(error_code_for),
        message: event
            .outward_message
            .clone()
            .unwrap_or_else(|| event.message.clone()),
        attempt: event.attempt,
        saksnummer: event
            .context
            .saksnummer
            .as_ref()
            .and_then(|value| Saksnummer::new(value.clone()).ok()),
        journalpost_id: event
            .context
            .journalpost_id
            .as_ref()
            .map(|value| JournalpostId(value.clone())),
        dokument_id: (!event.context.dokument_ids.is_empty()).then(|| {
            event
                .context
                .dokument_ids
                .iter()
                .cloned()
                .map(lib_schemas::skuffen::dokument::DokumentId)
                .collect()
        }),
        timestamp: event.timestamp.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::eksekvering::typer::{
        CommandLifecycleContext, CommandLifecycleMetadata, CommandStatus, CommandTypeCode,
    };
    use uuid::Uuid;

    fn lifecycle_event(
        stage: CommandStage,
        stage_status: CommandStageStatus,
    ) -> CommandLifecycleEvent {
        CommandLifecycleEvent {
            command_id: Uuid::parse_str("123e4567-e89b-12d3-a456-426614174100").unwrap(),
            correlation_id: Some(Uuid::parse_str("123e4567-e89b-12d3-a456-426614174101").unwrap()),
            command_type: CommandTypeCode::OpprettSak,
            status: CommandStatus::Pending,
            stage,
            stage_status,
            terminal: domain::eksekvering::typer::is_terminal(stage, stage_status),
            message: domain::eksekvering::typer::status_message(stage, stage_status),
            outward_message: None,
            error_code: None,
            detail: None,
            context: CommandLifecycleContext::default(),
            attempt: None,
            timestamp: Some("2026-01-02T03:04:05Z".to_string()),
        }
    }

    #[test]
    fn public_status_event_json_pins_phase_and_status_wire_values() {
        let cases = [
            (
                CommandStage::Mottatt,
                CommandStageStatus::Venter,
                "ingest",
                "pending",
                false,
            ),
            (
                CommandStage::Mottatt,
                CommandStageStatus::Ok,
                "ingest",
                "ok",
                false,
            ),
            (
                CommandStage::Validert,
                CommandStageStatus::Ok,
                "validate",
                "ok",
                false,
            ),
            (
                CommandStage::Validert,
                CommandStageStatus::Error,
                "validate",
                "error",
                true,
            ),
            (
                CommandStage::Utfores,
                CommandStageStatus::Venter,
                "execution",
                "pending",
                false,
            ),
            (
                CommandStage::Utfores,
                CommandStageStatus::Blocked,
                "execution",
                "blocked",
                false,
            ),
            (
                CommandStage::Utfores,
                CommandStageStatus::Retrying,
                "execution",
                "retrying",
                false,
            ),
            (
                CommandStage::Utfores,
                CommandStageStatus::Ok,
                "execution",
                "ok",
                true,
            ),
            (
                CommandStage::Utfores,
                CommandStageStatus::Error,
                "execution",
                "error",
                true,
            ),
        ];

        for (stage, stage_status, expected_phase, expected_status, expected_terminal) in cases {
            let outward = to_public_status_event(&lifecycle_event(stage, stage_status));
            let json = serde_json::to_value(outward).unwrap();

            assert_eq!(json["phase"], expected_phase);
            assert_eq!(json["status"], expected_status);
            assert_eq!(json["terminal"], expected_terminal);
        }
    }

    #[test]
    fn public_status_event_json_pins_error_code_wire_values() {
        let cases = [
            (StatusErrorCode::InvalidRequest, "INVALID_REQUEST"),
            (StatusErrorCode::NotFound, "NOT_FOUND"),
            (StatusErrorCode::Conflict, "CONFLICT"),
            (StatusErrorCode::PrerequisitePending, "PREREQUISITE_PENDING"),
            (
                StatusErrorCode::TemporaryUnavailable,
                "TEMPORARY_UNAVAILABLE",
            ),
            (StatusErrorCode::ProcessingFailed, "PROCESSING_FAILED"),
        ];

        for (error_code, expected_wire_value) in cases {
            let mut event = lifecycle_event(CommandStage::Utfores, CommandStageStatus::Error);
            event.error_code = Some(error_code);

            let json = serde_json::to_value(to_public_status_event(&event)).unwrap();

            assert_eq!(json["error_code"], expected_wire_value);
        }
    }

    #[test]
    fn converts_execution_ok_to_public_status_event() {
        let event = CommandLifecycleEvent::new(
            CommandLifecycleMetadata::new(
                Uuid::parse_str("123e4567-e89b-12d3-a456-426614174100").unwrap(),
                CommandTypeCode::OpprettSak,
            ),
            Some(Uuid::parse_str("123e4567-e89b-12d3-a456-426614174101").unwrap()),
            CommandStatus::Ok,
            CommandStage::Utfores,
            CommandStageStatus::Ok,
            None,
            Some("Sak opprettet.".to_string()),
            CommandLifecycleContext {
                saksnummer: Some("2026/123".to_string()),
                ..Default::default()
            },
            Some(1),
        );

        let outward = to_public_status_event(&event);

        assert_eq!(outward.phase, SkuffenStatusPhase::Execution);
        assert_eq!(outward.status, SkuffenStatus::Ok);
        assert!(outward.terminal);
        assert_eq!(outward.error_code, None);
        assert_eq!(outward.message, "utfores::ok");
        assert_eq!(outward.saksnummer.unwrap().as_str(), "2026/123");
    }

    #[test]
    fn does_not_expose_internal_detail_without_outward_message() {
        let event = CommandLifecycleEvent::new(
            CommandLifecycleMetadata::new(
                Uuid::new_v4(),
                CommandTypeCode::OpprettInterntNotatJournalpost,
            ),
            None,
            CommandStatus::Error,
            CommandStage::Utfores,
            CommandStageStatus::Error,
            Some(StatusErrorCode::ProcessingFailed),
            Some("Sikri responded with internal archive detail".to_string()),
            CommandLifecycleContext::default(),
            Some(3),
        );

        let outward = to_public_status_event(&event);

        assert_eq!(outward.message, "utfores::error");
        assert_ne!(
            outward.message,
            "Sikri responded with internal archive detail"
        );
    }

    #[test]
    fn uses_outward_message_when_provided() {
        let event = CommandLifecycleEvent::new(
            CommandLifecycleMetadata::new(
                Uuid::new_v4(),
                CommandTypeCode::OpprettInterntNotatJournalpost,
            ),
            None,
            CommandStatus::Error,
            CommandStage::Utfores,
            CommandStageStatus::Error,
            Some(StatusErrorCode::ProcessingFailed),
            Some("Sikri responded with internal archive detail".to_string()),
            CommandLifecycleContext::default(),
            Some(3),
        )
        .with_outward_message("Command execution failed.");

        let outward = to_public_status_event(&event);

        assert_eq!(outward.message, "Command execution failed.");
    }

    #[test]
    fn converts_blocked_to_client_safe_error_code() {
        let event = CommandLifecycleEvent::new(
            CommandLifecycleMetadata::new(
                Uuid::new_v4(),
                CommandTypeCode::OpprettInterntNotatJournalpost,
            ),
            None,
            CommandStatus::Blocked,
            CommandStage::Utfores,
            CommandStageStatus::Blocked,
            Some(StatusErrorCode::PrerequisitePending),
            Some("Saksnummer mangler".to_string()),
            CommandLifecycleContext::default(),
            Some(2),
        );

        let outward = to_public_status_event(&event);

        assert_eq!(outward.status, SkuffenStatus::Blocked);
        assert_eq!(
            outward.error_code,
            Some(SkuffenStatusErrorCode::PrerequisitePending)
        );
        assert!(!outward.terminal);
    }
}
