use tracing::{error, warn};
use uuid::Uuid;

use lib_schemas::skuffen::admin::AdminSakKeyV1;

use crate::nats::nats_response::NatsResponse;

/// `utfort_av` er selvdeklarert attribusjon. Grensen hindrer loggmisbruk.
pub(crate) const MAX_UTFORT_AV_BYTES: usize = 128;

pub(crate) const INVALID_REQUEST: &str = "Invalid request format";
pub(crate) const COMMAND_NOT_FOUND: &str = "Command not found";
pub(crate) const SAK_NOT_FOUND: &str = "Sak not found";
pub(crate) const RESPONSE_TOO_LARGE: &str = "Response too large";
pub(crate) const INTERNAL_ERROR: &str = "Internal error";
pub(crate) const COMMAND_DISCARDED: &str = "Command is discarded";
pub(crate) const ORIGINAL_NOT_AVAILABLE: &str = "Original command not available";
pub(crate) const UNRESOLVED_OUTCOME: &str = "Sak has operations with unresolved outcome";
pub(crate) const BUSY: &str = "Saken har operasjoner som utføres. Prøv igjen senere";

/// Svaret på en gyldig request, med det listeneren trenger for sluttloggen.
pub(crate) struct AdminSvar {
    pub(crate) payload: Vec<u8>,
    pub(crate) resultat: &'static str,
    pub(crate) utfort_av: String,
    pub(crate) sluttlogg: Sluttlogg,
}

/// Feltene i «utført»-linjen er kontrakt (repo_rules §5): read logger oppslaget
/// i sluttlinjen, skrivehandlingene logger utvalget allerede ved start.
pub(crate) enum Sluttlogg {
    Read {
        key_type: &'static str,
        lookup: String,
    },
    Retry,
    Forkast,
}

pub(crate) struct UgyldigRequest {
    pub(crate) payload: Vec<u8>,
}

impl UgyldigRequest {
    pub(crate) fn ny() -> Self {
        Self {
            payload: feilsvar(INVALID_REQUEST),
        }
    }
}

pub(crate) fn parse<T: serde::de::DeserializeOwned>(payload: &[u8]) -> Result<T, UgyldigRequest> {
    serde_json::from_slice(payload).map_err(|_| UgyldigRequest::ny())
}

pub(crate) fn feilsvar(message: &str) -> Vec<u8> {
    serde_json::to_vec(&NatsResponse::<()>::Error {
        message: message.to_string(),
    })
    .expect("statisk feilsvar er serialiserbart")
}

/// Hele `NatsResponse::Ok` måles mot NATS-grensen. Uten guarden ville
/// caller bare fått timeout når success-responsen ikke kan sendes.
pub(crate) fn serialiser_ok<T: serde::Serialize>(
    response: T,
    max_payload: usize,
) -> (Vec<u8>, &'static str) {
    match serde_json::to_vec(&NatsResponse::Ok(response)) {
        Ok(bytes) if bytes.len() > max_payload => {
            warn!(
                payload_size = bytes.len(),
                max_payload, "admin-svar overskrider NATS-grensen"
            );
            (feilsvar(RESPONSE_TOO_LARGE), "response_too_large")
        }
        Ok(bytes) => (bytes, "ok"),
        Err(err) => {
            error!(error = %err, "kunne ikke serialisere admin-svar");
            (feilsvar(INTERNAL_ERROR), "error")
        }
    }
}

/// `ArkivId` er fri tekst i kontrakten og kan inneholde historisk eller
/// uventet innhold; attribusjonsloggen bruker derfor bare key-typen.
pub(crate) fn nokkel_logg(key: &AdminSakKeyV1) -> (&'static str, String) {
    match key {
        AdminSakKeyV1::SkuffenId(id) => ("skuffen_id", uuid_logg(*id)),
        AdminSakKeyV1::ClientReference(id) => ("client_reference", uuid_logg(*id)),
        AdminSakKeyV1::ArkivId(_) => ("arkiv_id", String::new()),
    }
}

fn uuid_logg(id: Uuid) -> String {
    id.to_string()
}

/// Trimmer, avviser blank verdi og control characters, og setter en eksplisitt
/// øvre grense. Verdien sendes ikke inn i application-laget.
pub(crate) fn valider_utfort_av(verdi: &str) -> Result<String, UgyldigRequest> {
    let trimmet = verdi.trim();
    if trimmet.is_empty()
        || trimmet.len() > MAX_UTFORT_AV_BYTES
        || trimmet.chars().any(char::is_control)
    {
        return Err(UgyldigRequest::ny());
    }
    Ok(trimmet.to_string())
}
