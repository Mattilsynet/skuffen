use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

/// Tar imot en validert kommando og registrerer den i eksekveringssystemet.
///
/// Validering eier referanse- og regelkontroller, og kan kun bruke `id_mapping`
/// som lokal persistence. Denne use casen eier materialisering av
/// eksekveringssystemets state, registrering i `command_execution`, og utsending
/// av `utfores::venter` nar kommandoen faktisk ble registrert.
#[async_trait]
pub trait RegistrerIEksekveringssystemUseCase: Send + Sync {
    async fn handle(&self, envelope: &CommandEnvelope<Command>) -> Result<(), anyhow::Error>;
}
