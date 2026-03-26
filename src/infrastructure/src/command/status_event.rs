use domain::eksekvering::typer::{CommandLifecycleEvent, CommandStage, CommandStageStatus};
use lib_schemas::skuffen::journalpost::JournalpostId;
use lib_schemas::skuffen::sak::Saksnummer;
use lib_schemas::skuffen::status::{SkuffenStatus, SkuffenStatusEventV1, SkuffenStatusPhase};

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

pub fn to_public_status_event(event: &CommandLifecycleEvent) -> SkuffenStatusEventV1 {
    SkuffenStatusEventV1 {
        command_id: event.command_id,
        correlation_id: event.correlation_id,
        phase: phase_for(event.stage),
        status: status_for(event.stage_status),
        terminal: event.terminal,
        error_code: event.error_code.clone(),
        message: event.outward_message.clone().unwrap_or_else(|| {
            event
                .detail
                .clone()
                .unwrap_or_else(|| event.message.clone())
        }),
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
        CommandEntityType, CommandLifecycleContext, CommandLifecycleMetadata, CommandTypeCode,
    };
    use lib_schemas::skuffen::command::commands::CommandStatus;
    use lib_schemas::skuffen::status::SkuffenStatusErrorCode;
    use uuid::Uuid;

    #[test]
    fn converts_execution_ok_to_public_status_event() {
        let event = CommandLifecycleEvent::new(
            CommandLifecycleMetadata::new(
                Uuid::parse_str("123e4567-e89b-12d3-a456-426614174100").unwrap(),
                CommandTypeCode::OpprettSak,
                CommandEntityType::Sak,
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
        assert_eq!(outward.message, "Sak opprettet.");
        assert_eq!(outward.saksnummer.unwrap().as_str(), "2026/123");
    }

    #[test]
    fn converts_blocked_to_client_safe_error_code() {
        let event = CommandLifecycleEvent::new(
            CommandLifecycleMetadata::new(
                Uuid::new_v4(),
                CommandTypeCode::OpprettInterntNotatJournalpost,
                CommandEntityType::Journalpost,
            ),
            None,
            CommandStatus::Blocked,
            CommandStage::Utfores,
            CommandStageStatus::Blocked,
            Some(SkuffenStatusErrorCode::PrerequisitePending),
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
