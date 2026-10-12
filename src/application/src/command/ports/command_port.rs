use async_trait::async_trait;
use domain::eksekvering::id::SkuffenSakId;
use uuid::Uuid;

use crate::command::{Command, CommandEnvelope};

/// Idempotency-nøkkelen er `dispatchet_at`, ikke radens eksistens
/// (SKU-0016 R11): raden skrives ved mottak, milepælen etter vellykket
/// dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mottaksresultat {
    Ny,
    /// Dispatch fullførte ikke forrige gang. Skal dispatches på nytt.
    MottattIkkeDispatchet,
    /// Ekte duplikat.
    AlleredeDispatchet,
}

impl Mottaksresultat {
    pub fn maa_dispatches(self) -> bool {
        !matches!(self, Self::AlleredeDispatchet)
    }
}

#[async_trait]
pub trait CommandRepository: Send + Sync {
    /// Idempotent.
    async fn registrer_mottatt(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<Mottaksresultat, anyhow::Error>;

    async fn marker_dispatchet(&self, command_id: Uuid) -> Result<(), anyhow::Error>;

    /// Kobler kommandoen til saken den gjelder. Første kobling vinner, så en
    /// replay ikke kan flytte kommandoen til en annen sak.
    async fn knytt_sak(&self, command_id: Uuid, sak_id: SkuffenSakId) -> Result<(), anyhow::Error>;
}
