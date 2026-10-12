//! Admin retry over NATS core request-reply.
//!
//! Separate subjects og queue groups for saksutvalg og kommandoliste.
//! Skrivetilgang gis separat fra admin read i NATS.

use std::sync::Arc;

use application::admin::services::admin_retry_service::{AdminRetryError, AdminRetryService};
use futures::StreamExt;
use lib_schemas::skuffen::admin::AdminSakKeyV1;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, error, info, warn};
use uuid::Uuid;

use crate::admin::mapping::til_sak_nokkel;
use crate::admin::nats::listener::{
    AdminMessage, AdminTransport, COMMAND_NOT_FOUND, INTERNAL_ERROR, INVALID_REQUEST,
    SAK_NOT_FOUND, UgyldigRequest, feilsvar, nokkel_logg, valider_utfort_av,
};
use crate::http::helse::Helse;
use crate::nats::nats_response::NatsResponse;
use crate::nats::supervisor::{RESTARTBUDSJETT, TaskSupervisor, tasknavn};

pub const ADMIN_WRITE_RETRY_SAK_SUBJECT: &str = "arkiv.admin.write.retry.sak";
pub const ADMIN_WRITE_RETRY_COMMANDS_SUBJECT: &str = "arkiv.admin.write.retry.commands";
const SAK_QUEUE_GROUP: &str = "skuffen-admin-write-retry-sak-v1";
const COMMANDS_QUEUE_GROUP: &str = "skuffen-admin-write-retry-commands-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetryAction {
    Sak,
    Commands,
}

impl RetryAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Sak => "write.retry.sak",
            Self::Commands => "write.retry.commands",
        }
    }

    fn subject(self) -> &'static str {
        match self {
            Self::Sak => ADMIN_WRITE_RETRY_SAK_SUBJECT,
            Self::Commands => ADMIN_WRITE_RETRY_COMMANDS_SUBJECT,
        }
    }

    fn queue_group(self) -> &'static str {
        match self {
            Self::Sak => SAK_QUEUE_GROUP,
            Self::Commands => COMMANDS_QUEUE_GROUP,
        }
    }
}

pub(crate) const ORIGINAL_NOT_AVAILABLE: &str = "Original command not available";
pub(crate) const UNRESOLVED_OUTCOME: &str = "Sak has operations with unresolved outcome";
pub(crate) const BUSY: &str = "Saken har operasjoner som utføres. Prøv igjen senere";

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

pub struct AdminRetryListener {
    transport: Arc<dyn AdminTransport>,
    service: Arc<AdminRetryService>,
    helse: Helse,
    shutdown: CancellationToken,
}

impl AdminRetryListener {
    pub fn new(
        transport: Arc<dyn AdminTransport>,
        service: Arc<AdminRetryService>,
        helse: Helse,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            transport,
            service,
            helse,
            shutdown,
        }
    }

    #[tracing::instrument(skip_all, name = "nats.admin_retry_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        TaskSupervisor::critical(tasknavn::ADMIN_RETRY_LISTENER, RESTARTBUDSJETT)
            .with_shutdown(self.shutdown.clone())
            .with_helse(&self.helse)
            .run(|| self.run_once())
            .await
    }

    pub async fn run_once(&self) -> anyhow::Result<()> {
        info!(
            sak_subject = ADMIN_WRITE_RETRY_SAK_SUBJECT,
            commands_subject = ADMIN_WRITE_RETRY_COMMANDS_SUBJECT,
            "admin retry listener starter"
        );
        tokio::try_join!(
            self.subscription_loop(RetryAction::Sak),
            self.subscription_loop(RetryAction::Commands),
        )?;
        Ok(())
    }

    async fn subscription_loop(&self, action: RetryAction) -> anyhow::Result<()> {
        let mut subscription = self
            .transport
            .queue_subscribe(action.subject(), action.queue_group())
            .await?;

        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => return Ok(()),
                message = subscription.next() => match message {
                    Some(message) => self.handle_message(action, message).await,
                    None => {
                        return Err(anyhow::anyhow!(
                            "admin subscription on {} ended unexpectedly",
                            action.subject()
                        ));
                    }
                },
            }
        }
    }

    async fn handle_message(&self, action: RetryAction, message: AdminMessage) {
        let span = tracing::info_span!("admin.write", admin_action = action.as_str());
        crate::telemetry::set_parent_on_span_from_nats_headers(&span, message.headers.as_ref());
        self.handle_i_span(action, message).instrument(span).await
    }

    async fn handle_i_span(&self, action: RetryAction, message: AdminMessage) {
        let Some(reply) = message.reply else {
            error!(
                admin_action = action.as_str(),
                "admin request uten reply subject"
            );
            return;
        };

        let request = match parse_request(action, &message.payload) {
            Ok(request) => request,
            Err(ugyldig) => {
                self.publiser(reply, ugyldig.payload).await;
                warn!(
                    admin_action = action.as_str(),
                    resultat = "invalid_request",
                    "admin request avvist ved wire-grensen"
                );
                return;
            }
        };

        // Attribusjon før sideeffekter, så handlingen er sporbar selv om
        // svaret aldri kommer frem.
        let svar = match request.mal {
            RetryMal::Commands(command_ids) => {
                info!(
                    admin_action = action.as_str(),
                    utfort_av = %request.utfort_av,
                    selection = "commands",
                    command_ids = ?command_ids,
                    "admin retry startet"
                );
                self.service.handle(command_ids).await
            }
            RetryMal::Sak(key) => {
                let (key_type, lookup) = nokkel_logg(&key);
                info!(
                    admin_action = action.as_str(),
                    utfort_av = %request.utfort_av,
                    selection = "sak",
                    key_type,
                    lookup,
                    "admin retry startet"
                );
                self.service.handle_sak(til_sak_nokkel(key)).await
            }
        };
        let (payload, resultat) = match svar {
            Ok(()) => serialiser_ok(),
            Err(err) => feil_til_svar(&err),
        };

        let resultat = if self.publiser(reply, payload).await {
            resultat
        } else {
            "error"
        };

        info!(
            admin_action = action.as_str(),
            utfort_av = %request.utfort_av,
            resultat,
            "admin retry utført"
        );
    }

    async fn publiser(&self, reply: String, payload: Vec<u8>) -> bool {
        match self.transport.publish(reply, payload).await {
            Ok(()) => true,
            Err(err) => {
                error!(error = %err, "kunne ikke publisere admin retry-svar");
                false
            }
        }
    }
}

struct GyldigRequest {
    utfort_av: String,
    mal: RetryMal,
}

enum RetryMal {
    Commands(Vec<Uuid>),
    Sak(AdminSakKeyV1),
}

fn parse_request(action: RetryAction, payload: &[u8]) -> Result<GyldigRequest, UgyldigRequest> {
    let (utfort_av, mal) = match action {
        RetryAction::Commands => {
            let request: AdminRetryCommandsRequestV1 =
                serde_json::from_slice(payload).map_err(|_| UgyldigRequest::ny())?;
            (
                valider_utfort_av(&request.utfort_av)?,
                RetryMal::Commands(request.command_ids),
            )
        }
        RetryAction::Sak => {
            let request: AdminRetrySakRequestV1 =
                serde_json::from_slice(payload).map_err(|_| UgyldigRequest::ny())?;
            (
                valider_utfort_av(&request.utfort_av)?,
                RetryMal::Sak(request.key),
            )
        }
    };
    Ok(GyldigRequest { utfort_av, mal })
}

fn serialiser_ok() -> (Vec<u8>, &'static str) {
    match serde_json::to_vec(&NatsResponse::Ok(())) {
        Ok(bytes) => (bytes, "ok"),
        Err(err) => {
            error!(error = %err, "kunne ikke serialisere admin retry-svar");
            (feilsvar(INTERNAL_ERROR), "error")
        }
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
        assert!(
            parse_request(
                RetryAction::Commands,
                json!({ "command_ids": [id] }).to_string().as_bytes()
            )
            .is_err()
        );
        assert!(
            parse_request(
                RetryAction::Commands,
                json!({ "utfort_av": "  ", "command_ids": [id] })
                    .to_string()
                    .as_bytes()
            )
            .is_err()
        );
        assert!(
            parse_request(
                RetryAction::Commands,
                json!({ "utfort_av": "op", "command_ids": [id], "sak": "x" })
                    .to_string()
                    .as_bytes()
            )
            .is_err()
        );
        let gyldig = parse_request(
            RetryAction::Commands,
            json!({ "utfort_av": " op ", "command_ids": [id] })
                .to_string()
                .as_bytes(),
        )
        .unwrap_or_else(|_| panic!("gyldig request"));
        assert_eq!(gyldig.utfort_av, "op");
        assert!(matches!(gyldig.mal, RetryMal::Commands(ids) if ids == vec![id]));
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
            let request = parse_request(RetryAction::Sak, wire.to_string().as_bytes())
                .unwrap_or_else(|_| panic!("gyldig saksrequest"));
            assert!(matches!(request.mal, RetryMal::Sak(key) if key == forventet));
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
            for action in [RetryAction::Sak, RetryAction::Commands] {
                assert!(
                    parse_request(action, wire.to_string().as_bytes()).is_err(),
                    "{action:?}: {wire}"
                );
            }
        }
    }

    #[test]
    fn ok_er_en_fast_unit_kvittering() {
        let (payload, resultat) = serialiser_ok();
        assert_eq!(payload, br#"{"status":"Ok","payload":null}"#);
        assert_eq!(resultat, "ok");
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

#[cfg(test)]
mod transport_tests;
