//! Admin-forkasting (SKU-0024): tar kommandoer ut av videre behandling.

use application::admin::services::admin_forkast_service::{AdminForkastError, AdminForkastService};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::admin::nats::rute::AdminRute;
use crate::admin::nats::svar::{
    AdminSvar, COMMAND_NOT_FOUND, INTERNAL_ERROR, INVALID_REQUEST, Sluttlogg, UgyldigRequest,
    feilsvar, parse, serialiser_ok, valider_utfort_av,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminForkastCommandsRequestV1 {
    pub utfort_av: String,
    pub command_ids: Vec<Uuid>,
}

/// Attribusjon logges før sideeffekter, så handlingen er sporbar selv om
/// svaret aldri kommer frem.
pub(crate) async fn commands(
    service: &AdminForkastService,
    payload: &[u8],
    max_payload: usize,
) -> Result<AdminSvar, UgyldigRequest> {
    let request: AdminForkastCommandsRequestV1 = parse(payload)?;
    let utfort_av = valider_utfort_av(&request.utfort_av)?;
    info!(
        admin_action = AdminRute::ForkastCommands.as_str(),
        utfort_av = %utfort_av,
        command_ids = ?request.command_ids,
        "admin forkast startet"
    );

    let (payload, resultat) = match service.handle(request.command_ids).await {
        Ok(()) => serialiser_ok((), max_payload),
        Err(err) => feil_til_svar(&err),
    };
    Ok(AdminSvar {
        payload,
        resultat,
        utfort_av,
        sluttlogg: Sluttlogg::Forkast,
    })
}

fn feil_til_svar(err: &AdminForkastError) -> (Vec<u8>, &'static str) {
    match err {
        AdminForkastError::UgyldigAntall => (feilsvar(INVALID_REQUEST), "invalid_request"),
        AdminForkastError::CommandNotFound(ids) => {
            warn!(command_ids = ?ids, "admin forkast: ukjente kommandoer");
            (feilsvar(COMMAND_NOT_FOUND), "not_found")
        }
        AdminForkastError::Teknisk(err) => {
            error!(error = %format!("{err:#}"), "admin forkast feilet");
            (feilsvar(INTERNAL_ERROR), "error")
        }
    }
}
