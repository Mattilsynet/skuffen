use async_trait::async_trait;

use application::command::ports::command_state_port::{
    ArkivSakTilstand, ArkivSakTilstandError, ArkivSakTilstandRepository,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct FakeArkivSakTilstandRepository;

#[async_trait]
impl ArkivSakTilstandRepository for FakeArkivSakTilstandRepository {
    async fn hent_sak_tilstand_fra_arkivet(
        &self,
        _saksnummer: &str,
    ) -> Result<ArkivSakTilstand, ArkivSakTilstandError> {
        Ok(ArkivSakTilstand { avsluttet: false })
    }
}
