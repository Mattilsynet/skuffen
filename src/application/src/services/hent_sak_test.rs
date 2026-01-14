#[cfg(test)]
mod tests {
    use crate::ports::use_cases::HentSakUseCase;
    use crate::services::hent_sak::{HentSakService, SakRepository};
    use async_trait::async_trait;
    use domain::model::sak::{Ordningsverdi, Sak, SakKey, Saksnummer, Saksstatus, Sakstittel};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use uuid::Uuid;

    struct FakeSakRepository {
        saker: Mutex<HashMap<SakKey, Sak>>,
    }

    #[async_trait]
    impl SakRepository for FakeSakRepository {
        async fn hent_sak(
            &self,
            key: SakKey,
            _inkluder_journalposter: bool,
        ) -> anyhow::Result<Sak> {
            let map = self.saker.lock().unwrap();
            match map.get(&key) {
                Some(sak) => Ok(sak.clone()),
                None => Err(anyhow::anyhow!("Sak ikke funnet")),
            }
        }
    }

    #[tokio::test]
    async fn hent_sak_ok() {
        let key = SakKey {
            skuffen_id: Uuid::new_v4(),
            arkiv_id: Some(Saksnummer::new("2021/1").unwrap()),
        };
        let sak = Sak {
            sakstittel: Sakstittel::try_from("Tittel").unwrap(),
            saksbehandler: "Me".to_string(),
            saksstatus: Saksstatus::UnderBehandling,
            tilgang: None,
            sak_key: key.clone(),
            kildesystem: "KS".to_string(),
            lukket: false,
            journalposter: vec![],
            ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
        };

        let mut map = HashMap::new();
        map.insert(key.clone(), sak.clone());
        let repo = FakeSakRepository {
            saker: Mutex::new(map),
        };
        let service = HentSakService::new(Box::new(repo));

        let result = service.handle(key, false).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), sak);
    }

    #[tokio::test]
    async fn hent_sak_not_found() {
        let key = SakKey {
            skuffen_id: Uuid::new_v4(),
            arkiv_id: Some(Saksnummer::new("2021/2").unwrap()),
        };
        let repo = FakeSakRepository {
            saker: Mutex::new(HashMap::new()),
        };
        let service = HentSakService::new(Box::new(repo));

        let result = service.handle(key, false).await;
        assert!(result.is_err());
        assert_eq!(result.err().unwrap().to_string(), "Sak ikke funnet");
    }
}
