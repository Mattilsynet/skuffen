use async_trait::async_trait;
use domain::model::sak::{Ordningsverdi, Sak, SakKey, Saksbehandler, Saksstatus, Sakstittel};

use application::query::services::hent_sak::SakRepository;

#[derive(Clone, Debug, Default)]
pub struct FakeSakRepository;

impl FakeSakRepository {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SakRepository for FakeSakRepository {
    async fn hent_sak(
        &self,
        key: SakKey,
        _inkluder_journalposter: bool,
    ) -> Result<Sak, anyhow::Error> {
        let saksbehandler = Saksbehandler::new("Z00000".to_string(), "42".to_string())?;
        Ok(Sak {
            client_reference: None,
            sakstittel: Sakstittel("Fake sak".to_string()),
            saksbehandler: saksbehandler.saksbehandler_id,
            saksstatus: Saksstatus::UnderBehandling,
            tilgang: None,
            sak_key: key,
            kildesystem: "SKUFFEN".to_string(),
            lukket: false,
            journalposter: vec![],
            ordningsverdi: Ordningsverdi::new("2026-1".to_string())?,
        })
    }
}
