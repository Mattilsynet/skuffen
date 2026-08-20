use async_trait::async_trait;
use domain::eksekvering::typer::{CommandStatus, Operasjonstatus};

/// Én statusstrøm (D31). Strømmen **er** loggen; en klient som vil ha
/// historikken lager en consumer med `DeliverPolicy::All`.
///
/// Det finnes bevisst ingen egen operasjon- eller loggstrøm, og ingen egen
/// `done`-strøm — statusstrømmen bærer terminal (D34).
#[async_trait]
pub trait StatusPublisher: Send + Sync {
    async fn publiser_command_status(&self, status: CommandStatus) -> Result<(), anyhow::Error>;

    async fn publiser_operasjonstatus(&self, status: Operasjonstatus) -> Result<(), anyhow::Error>;
}
