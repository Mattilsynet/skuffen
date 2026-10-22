//! Admin read (SKU-0018): autoritativ lokal reparasjonstilstand, aldri skriving.

use application::admin::services::admin_read_service::{AdminReadError, AdminReadService};
use lib_schemas::skuffen::admin::{HentAdminCommandRequestV1, HentAdminSakRequestV1};
use tracing::error;

use crate::admin::mapping;
use crate::admin::nats::svar::{
    AdminSvar, COMMAND_NOT_FOUND, INTERNAL_ERROR, SAK_NOT_FOUND, Sluttlogg, UgyldigRequest,
    feilsvar, nokkel_logg, parse, serialiser_ok, valider_utfort_av,
};

pub(crate) async fn hent_command(
    service: &AdminReadService,
    payload: &[u8],
    max_payload: usize,
) -> Result<AdminSvar, UgyldigRequest> {
    let request: HentAdminCommandRequestV1 = parse(payload)?;
    let utfort_av = valider_utfort_av(&request.utfort_av)?;

    let (payload, resultat) = match service.hent_command(request.command_id).await {
        Ok(command) => serialiser_ok(mapping::til_command_response(command), max_payload),
        Err(AdminReadError::CommandNotFound) => (feilsvar(COMMAND_NOT_FOUND), "not_found"),
        Err(err) => {
            error!(error = %err, "admin command-oppslag feilet");
            (feilsvar(INTERNAL_ERROR), "error")
        }
    };

    Ok(AdminSvar {
        payload,
        resultat,
        utfort_av,
        sluttlogg: Sluttlogg::Read {
            key_type: "command_id",
            lookup: request.command_id.to_string(),
        },
    })
}

pub(crate) async fn hent_sak(
    service: &AdminReadService,
    payload: &[u8],
    max_payload: usize,
) -> Result<AdminSvar, UgyldigRequest> {
    let request: HentAdminSakRequestV1 = parse(payload)?;
    let utfort_av = valider_utfort_av(&request.utfort_av)?;
    let (key_type, lookup) = nokkel_logg(&request.key);

    let (payload, resultat) = match service.hent_sak(mapping::til_sak_nokkel(request.key)).await {
        Ok(sak) => serialiser_ok(mapping::til_sak_response(sak), max_payload),
        Err(AdminReadError::SakNotFound) => (feilsvar(SAK_NOT_FOUND), "not_found"),
        Err(err) => {
            error!(error = %err, "admin sak-oppslag feilet");
            (feilsvar(INTERNAL_ERROR), "error")
        }
    };

    Ok(AdminSvar {
        payload,
        resultat,
        utfort_av,
        sluttlogg: Sluttlogg::Read { key_type, lookup },
    })
}
