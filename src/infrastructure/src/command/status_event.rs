use domain::eksekvering::typer::{CommandLifecycleContext, CommandLifecycleEvent};
use lib_schemas::skuffen::command::commands::CommandStatus;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StatusEventContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sak_client_reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saksnummer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalpost_client_reference: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub journalpost_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dokument_client_references: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dokument_ids: Vec<String>,
}

impl From<&CommandLifecycleContext> for StatusEventContext {
    fn from(context: &CommandLifecycleContext) -> Self {
        Self {
            sak_client_reference: context.sak_client_reference.clone(),
            saksnummer: context.saksnummer.clone(),
            journalpost_client_reference: context.journalpost_client_reference.clone(),
            journalpost_id: context.journalpost_id.clone(),
            dokument_client_references: context.dokument_client_references.clone(),
            dokument_ids: context.dokument_ids.clone(),
        }
    }
}

impl StatusEventContext {
    pub fn is_empty(&self) -> bool {
        self.sak_client_reference.is_none()
            && self.saksnummer.is_none()
            && self.journalpost_client_reference.is_none()
            && self.journalpost_id.is_none()
            && self.dokument_client_references.is_empty()
            && self.dokument_ids.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusEventMessage {
    pub command_id: Uuid,
    pub command_type: String,
    pub entity_type: String,
    pub status: CommandStatus,
    pub stage: String,
    pub stage_status: String,
    pub terminal: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<StatusEventContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

impl From<&CommandLifecycleEvent> for StatusEventMessage {
    fn from(event: &CommandLifecycleEvent) -> Self {
        Self {
            command_id: event.command_id,
            command_type: event.command_type.as_code().to_string(),
            entity_type: event.entity_type.as_code().to_string(),
            status: event.status.clone(),
            stage: event.stage.as_code().to_string(),
            stage_status: event.stage_status.as_code().to_string(),
            terminal: event.terminal,
            message: event.message.clone(),
            detail: event.detail.clone(),
            context: (!event.context.is_empty()).then(|| StatusEventContext::from(&event.context)),
            attempt: event.attempt,
            timestamp: event.timestamp.clone(),
        }
    }
}
