//! Admin-flaten over NATS core request-reply: én listener for read, retry og
//! forkast.
//!
//! Hver rute har sin egen subscription og løkke, så en treg skrivehandling
//! ikke holder igjen admin read. Listeneren er admin-lokal med vilje:
//! feilsemantikken her er kontrakt, og skal ikke kobles til den offentlige
//! query-listenerens interne feiltyper.

use std::sync::Arc;

use application::admin::services::admin_forkast_service::AdminForkastService;
use application::admin::services::admin_read_service::AdminReadService;
use application::admin::services::admin_retry_service::AdminRetryService;
use futures::StreamExt;
use futures::future::try_join_all;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, error, info, warn};

use crate::admin::nats::rute::AdminRute;
use crate::admin::nats::svar::{AdminSvar, Sluttlogg, UgyldigRequest};
use crate::admin::nats::transport::{AdminMessage, AdminTransport};
use crate::admin::nats::{forkast, read, retry};
use crate::http::helse::Helse;
use crate::nats::supervisor::{RESTARTBUDSJETT, TaskSupervisor, tasknavn};

pub struct AdminServices {
    pub read: Arc<AdminReadService>,
    pub retry: Arc<AdminRetryService>,
    pub forkast: Arc<AdminForkastService>,
}

pub struct AdminListener {
    transport: Arc<dyn AdminTransport>,
    services: AdminServices,
    helse: Helse,
    pub(super) shutdown: CancellationToken,
}

impl AdminListener {
    pub fn new(
        transport: Arc<dyn AdminTransport>,
        services: AdminServices,
        helse: Helse,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            transport,
            services,
            helse,
            shutdown,
        }
    }

    #[tracing::instrument(skip_all, name = "nats.admin_listener")]
    pub async fn run(&self) -> anyhow::Result<()> {
        TaskSupervisor::critical(tasknavn::ADMIN_LISTENER, RESTARTBUDSJETT)
            .with_shutdown(self.shutdown.clone())
            .with_helse(&self.helse)
            .run(|| self.run_once())
            .await
    }

    /// En avsluttet subscription returnerer `Err`, slik at `try_join_all` ikke
    /// venter for alltid og supervisoren restarter alle.
    pub async fn run_once(&self) -> anyhow::Result<()> {
        info!(
            subjects = ?AdminRute::ALLE.map(AdminRute::subject),
            "admin listener starter"
        );
        try_join_all(AdminRute::ALLE.map(|rute| self.subscription_loop(rute))).await?;
        Ok(())
    }

    async fn subscription_loop(&self, rute: AdminRute) -> anyhow::Result<()> {
        let mut subscription = self
            .transport
            .queue_subscribe(rute.subject(), rute.queue_group())
            .await?;

        loop {
            tokio::select! {
                _ = self.shutdown.cancelled() => return Ok(()),
                message = subscription.next() => match message {
                    Some(message) => self.handle_message(rute, message).await,
                    None => {
                        return Err(anyhow::anyhow!(
                            "admin subscription on {} ended unexpectedly",
                            rute.subject()
                        ));
                    }
                },
            }
        }
    }

    /// Spanet lages og får parent **før** det aktiveres. Settes parent først
    /// inne i en `#[instrument]`-kropp, er OTel-spanet allerede bygget, og
    /// innkommende `traceparent` blir stille ignorert.
    pub(super) async fn handle_message(&self, rute: AdminRute, message: AdminMessage) {
        let span = if rute.er_skriving() {
            tracing::info_span!("admin.write", admin_action = rute.as_str())
        } else {
            tracing::info_span!("admin.read", admin_action = rute.as_str())
        };
        crate::telemetry::set_parent_on_span_from_nats_headers(&span, message.headers.as_ref());
        self.handle_i_span(rute, message).instrument(span).await
    }

    async fn handle_i_span(&self, rute: AdminRute, message: AdminMessage) {
        let Some(reply) = message.reply else {
            // Uten reply subject kan requesten ikke besvares. Payload logges ikke.
            error!(
                admin_action = rute.as_str(),
                "admin request uten reply subject"
            );
            return;
        };

        let svar = match self.behandle(rute, &message.payload).await {
            Ok(svar) => svar,
            Err(ugyldig) => {
                self.publiser(rute, reply, ugyldig.payload).await;
                warn!(
                    admin_action = rute.as_str(),
                    resultat = "invalid_request",
                    "admin request avvist ved wire-grensen"
                );
                return;
            }
        };

        let resultat = if self.publiser(rute, reply, svar.payload).await {
            svar.resultat
        } else {
            "error"
        };
        logg_utfort(rute, &svar.utfort_av, &svar.sluttlogg, resultat);
    }

    async fn behandle(&self, rute: AdminRute, payload: &[u8]) -> Result<AdminSvar, UgyldigRequest> {
        let max_payload = self.transport.max_payload();
        let services = &self.services;
        match rute {
            AdminRute::HentCommand => {
                read::hent_command(&services.read, payload, max_payload).await
            }
            AdminRute::HentSak => read::hent_sak(&services.read, payload, max_payload).await,
            AdminRute::RetryCommands => {
                retry::commands(&services.retry, payload, max_payload).await
            }
            AdminRute::RetrySak => retry::sak(&services.retry, payload, max_payload).await,
            AdminRute::ForkastCommands => {
                forkast::commands(&services.forkast, payload, max_payload).await
            }
        }
    }

    async fn publiser(&self, rute: AdminRute, reply: String, payload: Vec<u8>) -> bool {
        match self.transport.publish(reply, payload).await {
            Ok(()) => true,
            Err(err) => {
                error!(
                    admin_action = rute.as_str(),
                    error = %err,
                    "kunne ikke publisere admin-svar"
                );
                false
            }
        }
    }
}

/// Attribusjonslogg, ikke autentisert audit-logg. Ingen response-data.
fn logg_utfort(rute: AdminRute, utfort_av: &str, sluttlogg: &Sluttlogg, resultat: &str) {
    let admin_action = rute.as_str();
    match sluttlogg {
        Sluttlogg::Read { key_type, lookup } => info!(
            admin_action,
            utfort_av = %utfort_av,
            lookup = %lookup,
            key_type,
            resultat,
            "admin read utført"
        ),
        Sluttlogg::Retry => info!(
            admin_action,
            utfort_av = %utfort_av,
            resultat,
            "admin retry utført"
        ),
        Sluttlogg::Forkast => info!(
            admin_action,
            utfort_av = %utfort_av,
            resultat,
            "admin forkast utført"
        ),
    }
}
