use domain::eksekvering::typer::CommandStageStatus;
use lib_schemas::skuffen::command::commands::CommandStatus;
use lib_schemas::skuffen::status::SkuffenStatusErrorCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleDecision {
    pub status: CommandStatus,
    pub stage_status: CommandStageStatus,
    pub detail: Option<String>,
    pub error_code: Option<SkuffenStatusErrorCode>,
}

impl LifecycleDecision {
    pub fn ok(detail: Option<String>) -> Self {
        Self {
            status: CommandStatus::Ok,
            stage_status: CommandStageStatus::Ok,
            detail,
            error_code: None,
        }
    }

    pub fn blocked(detail: impl Into<String>, error_code: Option<SkuffenStatusErrorCode>) -> Self {
        Self {
            status: CommandStatus::Blocked,
            stage_status: CommandStageStatus::Blocked,
            detail: Some(detail.into()),
            error_code,
        }
    }

    pub fn retrying(detail: impl Into<String>, error_code: Option<SkuffenStatusErrorCode>) -> Self {
        Self {
            status: CommandStatus::Retrying,
            stage_status: CommandStageStatus::Retrying,
            detail: Some(detail.into()),
            error_code,
        }
    }

    pub fn error(detail: impl Into<String>, error_code: Option<SkuffenStatusErrorCode>) -> Self {
        Self {
            status: CommandStatus::Error,
            stage_status: CommandStageStatus::Error,
            detail: Some(detail.into()),
            error_code,
        }
    }
}
