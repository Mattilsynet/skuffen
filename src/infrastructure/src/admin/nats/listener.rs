//! Admin read request-reply over NATS core.
//!
//! Listeneren er admin-lokal med vilje: feilsemantikken her er kontrakt, og
//! skal ikke kobles til den offentlige query-listenerens interne feiltyper.

use std::pin::Pin;
use std::sync::Arc;

use application::admin::services::admin_read_service::{AdminReadError, AdminReadService};
use async_nats::HeaderMap;
use async_trait::async_trait;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use lib_schemas::skuffen::admin::{HentAdminCommandRequestV1, HentAdminSakRequestV1};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, error, info, warn};
use uuid::Uuid;

use crate::admin::mapping;
use crate::nats::client::NatsClient;
use crate::nats::nats_response::NatsResponse;
use crate::nats::supervisor::TaskSupervisor;

pub const ADMIN_READ_COMMAND_HENT_SUBJECT: &str = "arkiv.admin.read.command.hent";
pub const ADMIN_READ_SAK_HENT_SUBJECT: &str = "arkiv.admin.read.sak.hent";

/// Stabile queue groups, så bare én instans svarer under deploy-overlapp.
const COMMAND_QUEUE_GROUP: &str = "skuffen-admin-read-command-hent-v1";
const SAK_QUEUE_GROUP: &str = "skuffen-admin-read-sak-hent-v1";

/// `utfort_av` er selvdeklarert attribusjon. Grensen hindrer loggmisbruk.
const MAX_UTFORT_AV_BYTES: usize = 128;

const INVALID_REQUEST: &str = "Invalid request format";
const COMMAND_NOT_FOUND: &str = "Command not found";
const SAK_NOT_FOUND: &str = "Sak not found";
const RESPONSE_TOO_LARGE: &str = "Response too large";
const INTERNAL_ERROR: &str = "Internal error";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminAction {
    HentCommand,
    HentSak,
}

impl AdminAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::HentCommand => "read.command.hent",
            Self::HentSak => "read.sak.hent",
        }
    }

    fn subject(self) -> &'static str {
        match self {
            Self::HentCommand => ADMIN_READ_COMMAND_HENT_SUBJECT,
            Self::HentSak => ADMIN_READ_SAK_HENT_SUBJECT,
        }
    }

    fn queue_group(self) -> &'static str {
        match self {
            Self::HentCommand => COMMAND_QUEUE_GROUP,
            Self::HentSak => SAK_QUEUE_GROUP,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdminResultat {
    Ok,
    NotFound,
    Error,
    ResponseTooLarge,
}

impl AdminResultat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NotFound => "not_found",
            Self::Error => "error",
            Self::ResponseTooLarge => "response_too_large",
        }
    }
}

/// Én innkommende admin-request, uavhengig av NATS-klienten.
pub struct AdminMessage {
    pub reply: Option<String>,
    pub headers: Option<HeaderMap>,
    pub payload: Bytes,
}

pub type AdminMessageStream = Pin<Box<dyn Stream<Item = AdminMessage> + Send>>;

/// Liten grense rundt subscribe/publish, slik at handler- og
/// subscription-oppførsel kan testes uten en NATS-server.
#[async_trait]
pub trait AdminTransport: Send + Sync {
    async fn queue_subscribe(
        &self,
        subject: &'static str,
        queue_group: &'static str,
    ) -> anyhow::Result<AdminMessageStream>;

    async fn publish(&self, reply: String, payload: Vec<u8>) -> anyhow::Result<()>;

    fn max_payload(&self) -> usize;
}

pub struct NatsAdminTransport {
    client: NatsClient,
}

impl NatsAdminTransport {
    pub fn new(client: NatsClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl AdminTransport for NatsAdminTransport {
    async fn queue_subscribe(
        &self,
        subject: &'static str,
        queue_group: &'static str,
    ) -> anyhow::Result<AdminMessageStream> {
        let subscription = self
            .client
            .inner()
            .queue_subscribe(subject, queue_group.to_string())
            .await?;
        Ok(Box::pin(subscription.map(|message| AdminMessage {
            reply: message.reply.map(|reply| reply.to_string()),
            headers: message.headers,
            payload: message.payload,
        })))
    }

    async fn publish(&self, reply: String, payload: Vec<u8>) -> anyhow::Result<()> {
        self.client
            .inner()
            .publish(reply, payload.into())
            .await
            .map_err(anyhow::Error::from)
    }

    fn max_payload(&self) -> usize {
        self.client.inner().max_payload()
    }
}

pub struct AdminListener {
    transport: Arc<dyn AdminTransport>,
    service: Arc<AdminReadService>,
    shutdown: CancellationToken,
}

impl AdminListener {
    pub fn new(
        transport: Arc<dyn AdminTransport>,
        service: Arc<AdminReadService>,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            transport,
            service,
            shutdown,
        }
    }

    #[tracing::instrument(skip_all, name = "nats.admin_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        TaskSupervisor::background("admin_listener")
            .with_shutdown(self.shutdown.clone())
            .run(|| self.run_once())
            .await
    }

    /// En avsluttet subscription returnerer `Err`, slik at `try_join!` ikke
    /// venter for alltid og supervisoren restarter begge.
    pub async fn run_once(&self) -> anyhow::Result<()> {
        info!(
            command_subject = ADMIN_READ_COMMAND_HENT_SUBJECT,
            sak_subject = ADMIN_READ_SAK_HENT_SUBJECT,
            "admin read listener starter"
        );
        tokio::try_join!(
            self.subscription_loop(AdminAction::HentCommand),
            self.subscription_loop(AdminAction::HentSak),
        )?;
        Ok(())
    }

    async fn subscription_loop(&self, action: AdminAction) -> anyhow::Result<()> {
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

    /// Spanet lages og får parent **før** det aktiveres. Settes parent først
    /// inne i en `#[instrument]`-kropp, er OTel-spanet allerede bygget, og
    /// innkommende `traceparent` blir stille ignorert.
    async fn handle_message(&self, action: AdminAction, message: AdminMessage) {
        let span = tracing::info_span!("admin.read", admin_action = action.as_str());
        crate::telemetry::set_parent_on_span_from_nats_headers(&span, message.headers.as_ref());
        self.handle_i_span(action, message).instrument(span).await
    }

    async fn handle_i_span(&self, action: AdminAction, message: AdminMessage) {
        let Some(reply) = message.reply else {
            // Uten reply subject kan requesten ikke besvares. Payload logges ikke.
            error!(
                admin_action = action.as_str(),
                "admin request uten reply subject"
            );
            return;
        };

        let svar = match action {
            AdminAction::HentCommand => self.behandle_command(&message.payload).await,
            AdminAction::HentSak => self.behandle_sak(&message.payload).await,
        };

        let svar = match svar {
            Ok(svar) => svar,
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

        let publisert = self.publiser(reply, svar.payload).await;
        let resultat = if publisert {
            svar.resultat
        } else {
            AdminResultat::Error
        };

        // Attribusjonslogg, ikke autentisert audit-logg. Ingen response-data.
        info!(
            admin_action = action.as_str(),
            utfort_av = %svar.utfort_av,
            lookup = %svar.lookup,
            key_type = svar.key_type,
            resultat = resultat.as_str(),
            "admin read utført"
        );
    }

    async fn publiser(&self, reply: String, payload: Vec<u8>) -> bool {
        match self.transport.publish(reply, payload).await {
            Ok(()) => true,
            Err(err) => {
                error!(error = %err, "kunne ikke publisere admin-svar");
                false
            }
        }
    }

    async fn behandle_command(&self, payload: &[u8]) -> Result<AdminSvar, UgyldigRequest> {
        let request: HentAdminCommandRequestV1 =
            serde_json::from_slice(payload).map_err(|_| UgyldigRequest::ny())?;
        let utfort_av = valider_utfort_av(&request.utfort_av)?;

        let (payload, resultat) = match self.service.hent_command(request.command_id).await {
            Ok(command) => {
                let response = mapping::til_command_response(command);
                self.serialiser_ok(response)
            }
            Err(AdminReadError::CommandNotFound) => {
                (feilsvar(COMMAND_NOT_FOUND), AdminResultat::NotFound)
            }
            Err(err) => {
                error!(error = %err, "admin command-oppslag feilet");
                (feilsvar(INTERNAL_ERROR), AdminResultat::Error)
            }
        };

        Ok(AdminSvar {
            payload,
            resultat,
            utfort_av,
            lookup: request.command_id.to_string(),
            key_type: "command_id",
        })
    }

    async fn behandle_sak(&self, payload: &[u8]) -> Result<AdminSvar, UgyldigRequest> {
        let request: HentAdminSakRequestV1 =
            serde_json::from_slice(payload).map_err(|_| UgyldigRequest::ny())?;
        let utfort_av = valider_utfort_av(&request.utfort_av)?;
        let (key_type, lookup) = nokkel_logg(&request.key);

        let (payload, resultat) = match self
            .service
            .hent_sak(mapping::til_sak_nokkel(request.key))
            .await
        {
            Ok(sak) => {
                let response = mapping::til_sak_response(sak);
                self.serialiser_ok(response)
            }
            Err(AdminReadError::SakNotFound) => (feilsvar(SAK_NOT_FOUND), AdminResultat::NotFound),
            Err(err) => {
                error!(error = %err, "admin sak-oppslag feilet");
                (feilsvar(INTERNAL_ERROR), AdminResultat::Error)
            }
        };

        Ok(AdminSvar {
            payload,
            resultat,
            utfort_av,
            lookup,
            key_type,
        })
    }

    /// Hele `NatsResponse::Ok` måles mot NATS-grensen. Uten guarden ville
    /// caller bare fått timeout når success-responsen ikke kan sendes.
    fn serialiser_ok<T: serde::Serialize>(&self, response: T) -> (Vec<u8>, AdminResultat) {
        match serde_json::to_vec(&NatsResponse::Ok(response)) {
            Ok(bytes) if bytes.len() > self.transport.max_payload() => {
                warn!(
                    payload_size = bytes.len(),
                    max_payload = self.transport.max_payload(),
                    "admin-svar overskrider NATS-grensen"
                );
                (
                    feilsvar(RESPONSE_TOO_LARGE),
                    AdminResultat::ResponseTooLarge,
                )
            }
            Ok(bytes) => (bytes, AdminResultat::Ok),
            Err(err) => {
                error!(error = %err, "kunne ikke serialisere admin-svar");
                (feilsvar(INTERNAL_ERROR), AdminResultat::Error)
            }
        }
    }
}

struct AdminSvar {
    payload: Vec<u8>,
    resultat: AdminResultat,
    utfort_av: String,
    lookup: String,
    key_type: &'static str,
}

struct UgyldigRequest {
    payload: Vec<u8>,
}

impl UgyldigRequest {
    fn ny() -> Self {
        Self {
            payload: feilsvar(INVALID_REQUEST),
        }
    }
}

fn feilsvar(message: &str) -> Vec<u8> {
    serde_json::to_vec(&NatsResponse::<()>::Error {
        message: message.to_string(),
    })
    .expect("statisk feilsvar er serialiserbart")
}

/// `ArkivId` er fri tekst i kontrakten og kan inneholde historisk eller
/// uventet innhold; attribusjonsloggen bruker derfor bare key-typen.
fn nokkel_logg(key: &lib_schemas::skuffen::admin::AdminSakKeyV1) -> (&'static str, String) {
    use lib_schemas::skuffen::admin::AdminSakKeyV1;
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
fn valider_utfort_av(verdi: &str) -> Result<String, UgyldigRequest> {
    let trimmet = verdi.trim();
    if trimmet.is_empty()
        || trimmet.len() > MAX_UTFORT_AV_BYTES
        || trimmet.chars().any(char::is_control)
    {
        return Err(UgyldigRequest::ny());
    }
    Ok(trimmet.to_string())
}

#[cfg(test)]
mod tests;
