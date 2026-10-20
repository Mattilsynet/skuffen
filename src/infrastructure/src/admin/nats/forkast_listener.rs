//! Admin-forkasting over NATS core request-reply (SKU-0024).
//!
//! Eget subject og egen queue group. Skrivetilgang gis separat fra admin read
//! og admin retry i NATS.

use std::sync::Arc;

use application::admin::services::admin_forkast_service::{AdminForkastError, AdminForkastService};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, error, info, warn};
use uuid::Uuid;

use crate::admin::nats::listener::{
    AdminMessage, AdminTransport, COMMAND_NOT_FOUND, INTERNAL_ERROR, INVALID_REQUEST,
    UgyldigRequest, feilsvar, valider_utfort_av,
};
use crate::http::helse::Helse;
use crate::nats::nats_response::NatsResponse;
use crate::nats::supervisor::{RESTARTBUDSJETT, TaskSupervisor, tasknavn};

pub const ADMIN_WRITE_FORKAST_COMMANDS_SUBJECT: &str = "arkiv.admin.write.forkast.commands";
const QUEUE_GROUP: &str = "skuffen-admin-write-forkast-commands-v1";
const ADMIN_ACTION: &str = "write.forkast.commands";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdminForkastCommandsRequestV1 {
    pub utfort_av: String,
    pub command_ids: Vec<Uuid>,
}

pub struct AdminForkastListener {
    transport: Arc<dyn AdminTransport>,
    service: Arc<AdminForkastService>,
    helse: Helse,
    shutdown: CancellationToken,
}

impl AdminForkastListener {
    pub fn new(
        transport: Arc<dyn AdminTransport>,
        service: Arc<AdminForkastService>,
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

    #[tracing::instrument(skip_all, name = "nats.admin_forkast_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        TaskSupervisor::critical(tasknavn::ADMIN_FORKAST_LISTENER, RESTARTBUDSJETT)
            .with_shutdown(self.shutdown.clone())
            .with_helse(&self.helse)
            .run(|| self.run_once())
            .await
    }

    pub async fn run_once(&self) -> anyhow::Result<()> {
        info!(
            subject = ADMIN_WRITE_FORKAST_COMMANDS_SUBJECT,
            "admin forkast listener starter"
        );
        let mut subscription = self
            .transport
            .queue_subscribe(ADMIN_WRITE_FORKAST_COMMANDS_SUBJECT, QUEUE_GROUP)
            .await?;

        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => return Ok(()),
                message = subscription.next() => match message {
                    Some(message) => self.handle_message(message).await,
                    None => {
                        return Err(anyhow::anyhow!(
                            "admin subscription on {ADMIN_WRITE_FORKAST_COMMANDS_SUBJECT} ended unexpectedly"
                        ));
                    }
                },
            }
        }
    }

    async fn handle_message(&self, message: AdminMessage) {
        let span = tracing::info_span!("admin.write", admin_action = ADMIN_ACTION);
        crate::telemetry::set_parent_on_span_from_nats_headers(&span, message.headers.as_ref());
        self.handle_i_span(message).instrument(span).await
    }

    async fn handle_i_span(&self, message: AdminMessage) {
        let Some(reply) = message.reply else {
            error!(
                admin_action = ADMIN_ACTION,
                "admin request uten reply subject"
            );
            return;
        };

        let (utfort_av, command_ids) = match parse_request(&message.payload) {
            Ok(request) => request,
            Err(ugyldig) => {
                self.publiser(reply, ugyldig.payload).await;
                warn!(
                    admin_action = ADMIN_ACTION,
                    resultat = "invalid_request",
                    "admin request avvist ved wire-grensen"
                );
                return;
            }
        };

        // Attribusjon før sideeffekter, så handlingen er sporbar selv om
        // svaret aldri kommer frem.
        info!(
            admin_action = ADMIN_ACTION,
            utfort_av = %utfort_av,
            command_ids = ?command_ids,
            "admin forkast startet"
        );

        let (payload, resultat) = match self.service.handle(command_ids).await {
            Ok(()) => serialiser_ok(),
            Err(err) => feil_til_svar(&err),
        };
        let resultat = if self.publiser(reply, payload).await {
            resultat
        } else {
            "error"
        };

        info!(
            admin_action = ADMIN_ACTION,
            utfort_av = %utfort_av,
            resultat,
            "admin forkast utført"
        );
    }

    async fn publiser(&self, reply: String, payload: Vec<u8>) -> bool {
        match self.transport.publish(reply, payload).await {
            Ok(()) => true,
            Err(err) => {
                error!(error = %err, "kunne ikke publisere admin forkast-svar");
                false
            }
        }
    }
}

fn parse_request(payload: &[u8]) -> Result<(String, Vec<Uuid>), UgyldigRequest> {
    let request: AdminForkastCommandsRequestV1 =
        serde_json::from_slice(payload).map_err(|_| UgyldigRequest::ny())?;
    Ok((valider_utfort_av(&request.utfort_av)?, request.command_ids))
}

fn serialiser_ok() -> (Vec<u8>, &'static str) {
    match serde_json::to_vec(&NatsResponse::Ok(())) {
        Ok(bytes) => (bytes, "ok"),
        Err(err) => {
            error!(error = %err, "kunne ikke serialisere admin forkast-svar");
            (feilsvar(INTERNAL_ERROR), "error")
        }
    }
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
