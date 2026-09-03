use crate::command::status_event::{
    command_subject, operasjon_subject, to_public_command_status, to_public_operasjonstatus,
};
use crate::nats::client::NatsClient;
use application::command::ports::status_publisher_port::StatusPublisher;

use async_nats::jetstream::{self, message::PublishMessage};
use async_trait::async_trait;
use domain::eksekvering::typer::{CommandStatus, Operasjonstatus};
use tracing::{Span, error, info};

/// Én statusstrøm. Strømmen **er** loggen — en klient som vil ha historikken
/// lager en consumer med `DeliverPolicy::All`.
///
/// Strømmen er at-least-once og deduplisert av ingen (SKU-0020 R5). En nøkkel
/// koblet til `attempt_no` ga en illusjon om exactly-once innenfor et
/// udokumentert tominuttersvindu, og kolliderte for de fire hendelsene som
/// deler `attempt_no = 0`.
///
/// Strømmen opprettes i `prepare_runtime`, ikke her.
#[derive(Clone)]
pub struct NatsStatusPublisher {
    jetstream: jetstream::Context,
}

impl NatsStatusPublisher {
    pub fn new(client: NatsClient) -> Self {
        Self {
            jetstream: jetstream::new(client.inner().clone()),
        }
    }

    /// Uten outbox er loggen eneste spor når publiseringen feiler. Utfallet
    /// er avgjort og skrevet i databasen; klienten er den som ikke får vite
    /// det.
    async fn publiser(&self, subject: String, payload: Vec<u8>) -> Result<(), anyhow::Error> {
        Span::current().record("subject", tracing::field::display(subject.as_str()));

        let mut message = PublishMessage::build().payload(payload.into());
        if let Some(headers) = crate::telemetry::trace_headers() {
            message = message.headers(headers);
        }

        match self.jetstream.send_publish(subject, message).await {
            Ok(kvittering) => kvittering.await.map(|_| ()).map_err(|err| {
                error!(error = %err, "statushendelse ble ikke bekreftet av JetStream");
                anyhow::Error::new(err)
            }),
            Err(err) => {
                error!(error = %err, "statushendelse kunne ikke publiseres");
                Err(anyhow::Error::new(err))
            }
        }
    }
}

#[async_trait]
impl StatusPublisher for NatsStatusPublisher {
    #[tracing::instrument(
        skip_all,
        name = "nats.publish.command_status",
        fields(
            command_id = %status.command_id,
            correlation_id = tracing::field::Empty,
            hendelse = status.hendelse.as_code(),
            terminal = status.terminal,
            subject = tracing::field::Empty
        )
    )]
    async fn publiser_command_status(&self, status: CommandStatus) -> Result<(), anyhow::Error> {
        crate::telemetry::record_correlation_id(status.correlation_id);
        let subject = command_subject(status.command_id);
        let payload = serde_json::to_vec(&to_public_command_status(&status))?;
        // Statusstrømmen er kommandoens ytre fortelling. Den samme milepælen
        // logges, slik at et loggsøk på correlation_id gir hele forløpet uten
        // at man må lese NATS.
        info!(
            hendelse = status.hendelse.as_code(),
            error_code = status.error_code.map(|kode| kode.as_code()),
            "kommandostatus publisert"
        );
        self.publiser(subject, payload).await
    }

    #[tracing::instrument(
        skip_all,
        name = "nats.publish.operasjonstatus",
        fields(
            command_id = %status.command_id,
            correlation_id = tracing::field::Empty,
            operasjon_id = %status.operasjon_id.0,
            operasjonstype = status.operasjonstype.as_code(),
            hendelse = status.hendelse.as_code(),
            attempt_no = status.attempt_no,
            subject = tracing::field::Empty
        )
    )]
    async fn publiser_operasjonstatus(&self, status: Operasjonstatus) -> Result<(), anyhow::Error> {
        crate::telemetry::record_correlation_id(status.correlation_id);
        let subject = operasjon_subject(status.command_id, status.operasjon_id.0);
        let payload = serde_json::to_vec(&to_public_operasjonstatus(&status))?;
        info!(
            error_code = status.error_code.map(|kode| kode.as_code()),
            terminal = status.terminal,
            "operasjonstatus publisert"
        );
        self.publiser(subject, payload).await
    }
}
