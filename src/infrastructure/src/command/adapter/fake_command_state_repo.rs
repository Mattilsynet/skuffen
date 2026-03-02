use async_trait::async_trait;

use application::command::ports::command_state_port::{
    CommandStateError, CommandStateRepository, SakState,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct FakeCommandStateRepository;

#[async_trait]
impl CommandStateRepository for FakeCommandStateRepository {
    async fn hent_sak_state(&self, _saksnummer: &str) -> Result<SakState, CommandStateError> {
        Ok(SakState { avsluttet: false })
    }
}
