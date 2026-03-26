use domain::eksekvering::typer::{
    status_event, CommandLifecycleContext, CommandLifecycleEvent, CommandStage, CommandStageStatus,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope, CommandStatus};
use lib_schemas::skuffen::status::SkuffenStatusErrorCode;

fn outward_message(stage: CommandStage, stage_status: CommandStageStatus) -> &'static str {
    match (stage, stage_status) {
        (CommandStage::Mottatt, CommandStageStatus::Ok) => "Request accepted for processing.",
        (CommandStage::Validert, CommandStageStatus::Ok) => "Request validated successfully.",
        (CommandStage::Utfores, CommandStageStatus::Venter) => "Command is queued for execution.",
        _ => unreachable!("outward_message only supports lifecycle states without explicit detail"),
    }
}

pub fn mottatt_event(
    envelope: &CommandEnvelope<Command>,
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
    envelope: &CommandEnvelope<Command>,
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
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    error_code: Option<SkuffenStatusErrorCode>,
    context: CommandLifecycleContext,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Blocked,
        CommandStage::Validert,
        CommandStageStatus::Blocked,
        error_code.or(Some(SkuffenStatusErrorCode::PrerequisitePending)),
        Some(detail.into()),
        context,
        None,
    )
}

pub fn validert_retrying_event(
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    error_code: Option<SkuffenStatusErrorCode>,
    context: CommandLifecycleContext,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Retrying,
        CommandStage::Validert,
        CommandStageStatus::Retrying,
        error_code.or(Some(SkuffenStatusErrorCode::TemporaryUnavailable)),
        Some(detail.into()),
        context,
        None,
    )
}

pub fn validert_error_event(
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    error_code: Option<SkuffenStatusErrorCode>,
    context: CommandLifecycleContext,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Error,
        CommandStage::Validert,
        CommandStageStatus::Error,
        error_code.or(Some(SkuffenStatusErrorCode::InvalidRequest)),
        Some(detail.into()),
        context,
        None,
    )
}

pub fn utfores_venter_event(
    envelope: &CommandEnvelope<Command>,
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
    envelope: &CommandEnvelope<Command>,
    detail: Option<String>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    let outward_message = detail
        .clone()
        .unwrap_or_else(|| "Command executed successfully.".to_string());
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
    .with_outward_message(outward_message)
}

pub fn utfores_retrying_event(
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    error_code: Option<SkuffenStatusErrorCode>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    let detail = detail.into();
    status_event(
        envelope,
        CommandStatus::Retrying,
        CommandStage::Utfores,
        CommandStageStatus::Retrying,
        error_code.or(Some(SkuffenStatusErrorCode::TemporaryUnavailable)),
        Some(detail.clone()),
        context,
        attempt,
    )
    .with_outward_message(detail)
}

pub fn utfores_blocked_event(
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    error_code: Option<SkuffenStatusErrorCode>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    let detail = detail.into();
    status_event(
        envelope,
        CommandStatus::Blocked,
        CommandStage::Utfores,
        CommandStageStatus::Blocked,
        error_code.or(Some(SkuffenStatusErrorCode::PrerequisitePending)),
        Some(detail.clone()),
        context,
        attempt,
    )
    .with_outward_message(detail)
}

pub fn utfores_error_event(
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    error_code: Option<SkuffenStatusErrorCode>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    let detail = detail.into();
    status_event(
        envelope,
        CommandStatus::Error,
        CommandStage::Utfores,
        CommandStageStatus::Error,
        error_code.or(Some(SkuffenStatusErrorCode::ProcessingFailed)),
        Some(detail.clone()),
        context,
        attempt,
    )
    .with_outward_message(detail)
}
