use async_trait::async_trait;
use uuid::Uuid;

use crate::command::{Command, CommandEnvelope};

/// Hva mottaksjournalen sier om en kommando vi nettopp fikk inn.
///
/// Idempotency-nøkkelen er `dispatchet_at`, ikke radens eksistens
/// (SKU-0016 R11). Raden skrives ved mottak; milepælen settes først etter
/// vellykket dispatch. Uten det skillet gir en dispatch-feil etterfulgt av
/// klient-retry en OK-kvittering for en kommando som aldri ble sendt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mottaksresultat {
    /// Aldri sett før.
    Ny,
    /// Mottatt tidligere, men dispatch fullførte ikke. Skal dispatches på nytt.
    MottattIkkeDispatchet,
    /// Allerede dispatchet. Ekte duplikat; skal hoppes over.
    AlleredeDispatchet,
}

impl Mottaksresultat {
    pub fn maa_dispatches(self) -> bool {
        !matches!(self, Self::AlleredeDispatchet)
    }
}

#[async_trait]
pub trait CommandRepository: Send + Sync {
    /// Skriver mottaksraden idempotent og rapporterer dispatch-milepælen.
    async fn registrer_mottatt(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<Mottaksresultat, anyhow::Error>;

    async fn marker_dispatchet(&self, command_id: Uuid) -> Result<(), anyhow::Error>;
}
