//! Admin retry (SKU-0023): slipper stoppet arbeid inn i ordinær behandling.

use application::admin::services::admin_retry_service::{AdminRetryError, AdminRetryService};
use lib_schemas::skuffen::admin::AdminSakKeyV1;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::admin::mapping::til_sak_nokkel;
use crate::admin::nats::rute::AdminRute;
use crate::admin::nats::svar::{
    AdminSvar, BUSY, COMMAND_DISCARDED, COMMAND_NOT_FOUND, INTERNAL_ERROR, INVALID_REQUEST,
    ORIGINAL_NOT_AVAILABLE, SAK_NOT_FOUND, Sluttlogg, UNRESOLVED_OUTCOME, UgyldigRequest, feilsvar,
    nokkel_logg, parse, serialiser_ok, valider_utfort_av,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminRetrySakRequestV1 {
    pub utfort_av: String,
    pub key: AdminSakKeyV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminRetryCommandsRequestV1 {
    pub utfort_av: String,
    pub command_ids: Vec<Uuid>,
}

/// Attribusjon logges før sideeffekter, så handlingen er sporbar selv om
/// svaret aldri kommer frem.
pub(crate) async fn commands(
    service: &AdminRetryService,
    payload: &[u8],
    max_payload: usize,
) -> Result<AdminSvar, UgyldigRequest> {
    let (utfort_av, command_ids) = parse_commands(payload)?;
    info!(
        admin_action = AdminRute::RetryCommands.as_str(),
        utfort_av = %utfort_av,
        selection = "commands",
        command_ids = ?command_ids,
        "admin retry startet"
    );
    let utfall = service.handle(command_ids).await;
    Ok(svar(utfort_av, utfall, max_payload))
}

pub(crate) async fn sak(
    service: &AdminRetryService,
    payload: &[u8],
    max_payload: usize,
) -> Result<AdminSvar, UgyldigRequest> {
    let (utfort_av, key) = parse_sak(payload)?;
    let (key_type, lookup) = nokkel_logg(&key);
    info!(
        admin_action = AdminRute::RetrySak.as_str(),
        utfort_av = %utfort_av,
        selection = "sak",
        key_type,
        lookup,
        "admin retry startet"
    );
    let utfall = service.handle_sak(til_sak_nokkel(key)).await;
    Ok(svar(utfort_av, utfall, max_payload))
}

fn parse_commands(payload: &[u8]) -> Result<(String, Vec<Uuid>), UgyldigRequest> {
    let request: AdminRetryCommandsRequestV1 = parse(payload)?;
    Ok((valider_utfort_av(&request.utfort_av)?, request.command_ids))
}

fn parse_sak(payload: &[u8]) -> Result<(String, AdminSakKeyV1), UgyldigRequest> {
    let request: AdminRetrySakRequestV1 = parse(payload)?;
    Ok((valider_utfort_av(&request.utfort_av)?, request.key))
}

fn svar(utfort_av: String, utfall: Result<(), AdminRetryError>, max_payload: usize) -> AdminSvar {
    let (payload, resultat) = match utfall {
        Ok(()) => serialiser_ok((), max_payload),
        Err(err) => feil_til_svar(&err),
    };
    AdminSvar {
        payload,
        resultat,
        utfort_av,
        sluttlogg: Sluttlogg::Retry,
    }
}

fn feil_til_svar(err: &AdminRetryError) -> (Vec<u8>, &'static str) {
    match err {
        AdminRetryError::TomForesporsel | AdminRetryError::ForMange => {
            (feilsvar(INVALID_REQUEST), "invalid_request")
        }
        AdminRetryError::CommandNotFound(ids) => {
            warn!(command_ids = ?ids, "admin retry: ukjente kommandoer");
            (feilsvar(COMMAND_NOT_FOUND), "not_found")
        }
        AdminRetryError::Forkastet(ids) => {
            warn!(command_ids = ?ids, "admin retry: kommandoene er forkastet");
            (feilsvar(COMMAND_DISCARDED), "discarded")
        }
        AdminRetryError::SakNotFound => (feilsvar(SAK_NOT_FOUND), "not_found"),
        AdminRetryError::OriginalIkkeTilgjengelig(ids) => {
            warn!(command_ids = ?ids, "admin retry: originalen finnes ikke lenger");
            (feilsvar(ORIGINAL_NOT_AVAILABLE), "original_not_available")
        }
        AdminRetryError::SakHarUavklartUtfall(saker) => {
            let saker: Vec<String> = saker.iter().map(|sak| sak.0.to_string()).collect();
            warn!(sak_ids = ?saker, "admin retry: saken har uavklart utfall");
            (feilsvar(UNRESOLVED_OUTCOME), "unresolved_outcome")
        }
        AdminRetryError::Opptatt => (feilsvar(BUSY), "busy"),
        AdminRetryError::Teknisk(err) => {
            error!(error = %format!("{err:#}"), "admin retry feilet");
            (feilsvar(INTERNAL_ERROR), "error")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_krever_utfort_av_og_avviser_ukjente_felt() {
        let id = Uuid::new_v4();
        let avvist = |wire: serde_json::Value| parse_commands(wire.to_string().as_bytes()).is_err();
        assert!(avvist(json!({ "command_ids": [id] })));
        assert!(avvist(json!({ "utfort_av": "  ", "command_ids": [id] })));
        assert!(avvist(
            json!({ "utfort_av": "op", "command_ids": [id], "sak": "x" })
        ));
        let (utfort_av, command_ids) = parse_commands(
            json!({ "utfort_av": " op ", "command_ids": [id] })
                .to_string()
                .as_bytes(),
        )
        .unwrap_or_else(|_| panic!("gyldig request"));
        assert_eq!(utfort_av, "op");
        assert_eq!(command_ids, vec![id]);
    }

    #[test]
    fn sak_request_bruker_admin_read_nokler_og_beholder_wire_formatet() {
        let id = Uuid::new_v4();
        for (key, forventet) in [
            (
                json!({"type": "skuffenId", "value": id}),
                AdminSakKeyV1::SkuffenId(id),
            ),
            (
                json!({"type": "clientReference", "value": id}),
                AdminSakKeyV1::ClientReference(id),
            ),
            (
                json!({"type": "arkivId", "value": "2026/12345"}),
                AdminSakKeyV1::ArkivId("2026/12345".to_string()),
            ),
        ] {
            let wire = json!({"utfort_av": "op", "key": key});
            let (_, key) = parse_sak(wire.to_string().as_bytes())
                .unwrap_or_else(|_| panic!("gyldig saksrequest"));
            assert_eq!(key, forventet);
            let dto: AdminRetrySakRequestV1 = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(dto).unwrap(), wire);
        }
        let wire = json!({"utfort_av": "op", "command_ids": [id]});
        let dto: AdminRetryCommandsRequestV1 = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(dto).unwrap(), wire);
    }

    #[test]
    fn request_krever_ett_entydig_maal() {
        let id = Uuid::new_v4();
        let key = json!({"type": "skuffenId", "value": id});
        let avvist_av_begge = |wire: &serde_json::Value| {
            let bytes = wire.to_string();
            parse_sak(bytes.as_bytes()).is_err() && parse_commands(bytes.as_bytes()).is_err()
        };
        for wire in [
            json!({"utfort_av": "op"}),
            json!({"utfort_av": "op", "command_ids": [id], "key": key}),
            json!({"utfort_av": "op", "command_ids": null}),
            json!({"utfort_av": "op", "key": null}),
            json!({"utfort_av": "op", "command_ids": [id], "key": null}),
            json!({"utfort_av": "op", "command_ids": null, "key": key}),
            json!({"utfort_av": "op", "key": key, "extra": true}),
            json!({"utfort_av": "op", "key": {"type": "skuffenId", "value": id, "extra": true}}),
            json!({"utfort_av": "op", "key": {"type": "skuffenId", "value": "ugyldig"}}),
            json!({"utfort_av": "op", "key": {"type": "ukjent", "value": id}}),
            json!({"key": key}),
            json!({"utfort_av": " ", "key": key}),
        ] {
            assert!(avvist_av_begge(&wire), "{wire}");
        }
        assert!(
            parse_sak(
                json!({"utfort_av": "op", "command_ids": [id]})
                    .to_string()
                    .as_bytes()
            )
            .is_err()
        );
        assert!(
            parse_commands(
                json!({"utfort_av": "op", "key": key})
                    .to_string()
                    .as_bytes()
            )
            .is_err()
        );
    }

    #[test]
    fn ok_er_en_fast_unit_kvittering() {
        let svar = svar("op".to_string(), Ok(()), usize::MAX);
        assert_eq!(svar.payload, br#"{"status":"Ok","payload":null}"#);
        assert_eq!(svar.resultat, "ok");
    }

    #[test]
    fn laaskonflikt_returnerer_vanlig_feilsvar_med_beskjed_om_nytt_forsok() {
        let (payload, _) = feil_til_svar(&AdminRetryError::Opptatt);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&payload).unwrap(),
            json!({
                "status": "Error",
                "payload": {
                    "message": "Saken har operasjoner som utføres. Prøv igjen senere"
                }
            })
        );
    }

    #[test]
    fn feilene_har_stabile_meldinger() {
        let melding = |err: AdminRetryError| {
            let (payload, _) = feil_til_svar(&err);
            serde_json::from_slice::<serde_json::Value>(&payload).unwrap()["payload"]["message"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(melding(AdminRetryError::TomForesporsel), INVALID_REQUEST);
        assert_eq!(melding(AdminRetryError::ForMange), INVALID_REQUEST);
        assert_eq!(melding(AdminRetryError::SakNotFound), SAK_NOT_FOUND);
        assert_eq!(
            melding(AdminRetryError::CommandNotFound(vec![])),
            COMMAND_NOT_FOUND
        );
        assert_eq!(
            melding(AdminRetryError::Forkastet(vec![])),
            COMMAND_DISCARDED
        );
        assert_eq!(
            melding(AdminRetryError::OriginalIkkeTilgjengelig(vec![])),
            ORIGINAL_NOT_AVAILABLE
        );
        assert_eq!(
            melding(AdminRetryError::SakHarUavklartUtfall(vec![])),
            UNRESOLVED_OUTCOME
        );
        assert_eq!(melding(AdminRetryError::Opptatt), BUSY);
        assert_eq!(
            melding(AdminRetryError::Teknisk(anyhow::anyhow!("db"))),
            INTERNAL_ERROR
        );
    }
}
