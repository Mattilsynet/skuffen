use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use async_trait::async_trait;

use application::command::ports::command_state_port::{
    ArkivSakTilstand, ArkivSakTilstandError, ArkivSakTilstandErrorKind, ArkivSakTilstandRepository,
};

/// Saker fake-arkivet har opprettet.
///
/// Delt med [`FakeArkivGateway`](super::fake_arkiv_gateway::FakeArkivGateway),
/// så fake-en kan svare «finnes ikke» på et ukjent saksnummer.
fn kjente_saker() -> &'static Mutex<HashSet<String>> {
    static SAKER: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SAKER.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn registrer_fake_sak(saksnummer: &str) {
    kjente_saker()
        .lock()
        .expect("fake sak-registry poisoned")
        .insert(saksnummer.to_string());
}

pub fn fake_sak_finnes(saksnummer: &str) -> bool {
    kjente_saker()
        .lock()
        .expect("fake sak-registry poisoned")
        .contains(saksnummer)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct FakeArkivSakTilstandRepository;

#[async_trait]
impl ArkivSakTilstandRepository for FakeArkivSakTilstandRepository {
    async fn hent_sak_tilstand_fra_arkivet(
        &self,
        saksnummer: &str,
    ) -> Result<ArkivSakTilstand, ArkivSakTilstandError> {
        if fake_sak_finnes(saksnummer) {
            Ok(ArkivSakTilstand { avsluttet: false })
        } else {
            Err(ArkivSakTilstandError {
                kind: ArkivSakTilstandErrorKind::Irrecoverable,
                message: format!("Sak {saksnummer} finnes ikke i arkivet"),
            })
        }
    }
}
