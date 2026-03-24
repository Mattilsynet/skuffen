use domain::eksekvering::typer::{
    status_event, CommandLifecycleContext, CommandLifecycleEvent, CommandStage, CommandStageStatus,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope, CommandStatus};

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
        context,
        None,
    )
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
        context,
        None,
    )
}

pub fn validert_blocked_event(
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    context: CommandLifecycleContext,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Blocked,
        CommandStage::Validert,
        CommandStageStatus::Blocked,
        Some(detail.into()),
        context,
        None,
    )
}

pub fn validert_retrying_event(
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    context: CommandLifecycleContext,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Retrying,
        CommandStage::Validert,
        CommandStageStatus::Retrying,
        Some(detail.into()),
        context,
        None,
    )
}

pub fn validert_error_event(
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    context: CommandLifecycleContext,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Error,
        CommandStage::Validert,
        CommandStageStatus::Error,
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
        context,
        attempt,
    )
}

pub fn utfores_ok_event(
    envelope: &CommandEnvelope<Command>,
    detail: Option<String>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Ok,
        CommandStage::Utfores,
        CommandStageStatus::Ok,
        detail,
        context,
        attempt,
    )
}

pub fn utfores_retrying_event(
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Retrying,
        CommandStage::Utfores,
        CommandStageStatus::Retrying,
        Some(detail.into()),
        context,
        attempt,
    )
}

pub fn utfores_blocked_event(
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Blocked,
        CommandStage::Utfores,
        CommandStageStatus::Blocked,
        Some(detail.into()),
        context,
        attempt,
    )
}

pub fn utfores_error_event(
    envelope: &CommandEnvelope<Command>,
    detail: impl Into<String>,
    context: CommandLifecycleContext,
    attempt: Option<u32>,
) -> CommandLifecycleEvent {
    status_event(
        envelope,
        CommandStatus::Error,
        CommandStage::Utfores,
        CommandStageStatus::Error,
        Some(detail.into()),
        context,
        attempt,
    )
}
