#[cfg(test)]
mod tests {
    use crate::query::ports::use_cases::HentSakUseCase;
    use crate::query::services::hent_sak::{HentSakService, SakRepository, SaksnummerOppslag};
    use async_trait::async_trait;
    use domain::model::sak::{Ordningsverdi, Sak, SakKey, Saksnummer, Saksstatus, Sakstittel};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    #[derive(Default)]
    struct Kall {
        repo: Vec<(Saksnummer, bool)>,
        oppslag: Vec<Uuid>,
    }

    struct RecordingSakRepository {
        kall: Arc<Mutex<Kall>>,
        finnes: bool,
    }

    #[async_trait]
    impl SakRepository for RecordingSakRepository {
        async fn hent_sak(
            &self,
            saksnummer: Saksnummer,
            inkluder_journalposter: bool,
        ) -> anyhow::Result<Sak> {
            self.kall
                .lock()
                .unwrap()
                .repo
                .push((saksnummer.clone(), inkluder_journalposter));
            if self.finnes {
                Ok(sak(saksnummer))
            } else {
                Err(anyhow::anyhow!("Sak ikke funnet"))
            }
        }
    }

    struct RecordingOppslag {
        kall: Arc<Mutex<Kall>>,
        saksnummer: Option<Saksnummer>,
    }

    #[async_trait]
    impl SaksnummerOppslag for RecordingOppslag {
        async fn saksnummer_for_client_reference(
            &self,
            client_reference: Uuid,
        ) -> anyhow::Result<Saksnummer> {
            self.kall.lock().unwrap().oppslag.push(client_reference);
            self.saksnummer
                .clone()
                .ok_or_else(|| anyhow::anyhow!("client_reference ikke funnet"))
        }
    }

    fn sak(saksnummer: Saksnummer) -> Sak {
        Sak {
            client_reference: None,
            sakstittel: Sakstittel("Test Sak 1".to_string()),
            saksbehandler: "Z99999".to_string(),
            saksbehandler_enhet: None,
            saksstatus: Saksstatus::UnderBehandling,
            tilgang: None,
            saksnummer,
            kildesystem: "SKUFFEN".to_string(),
            lukket: false,
            journalposter: Some(vec![]),
            ordningsverdi: Ordningsverdi::new("2021-1".to_string()).unwrap(),
        }
    }

    fn service(
        oppslag_svar: Option<Saksnummer>,
        sak_finnes: bool,
    ) -> (HentSakService, Arc<Mutex<Kall>>) {
        let kall = Arc::new(Mutex::new(Kall::default()));
        let service = HentSakService::new(
            Box::new(RecordingSakRepository {
                kall: kall.clone(),
                finnes: sak_finnes,
            }),
            Box::new(RecordingOppslag {
                kall: kall.clone(),
                saksnummer: oppslag_svar,
            }),
        );
        (service, kall)
    }

    #[tokio::test]
    async fn saksnummer_hentes_direkte_uten_lokalt_oppslag() {
        let saksnummer = Saksnummer::new("2026/12345").unwrap();
        let (service, kall) = service(None, true);

        let resultat = service
            .handle(SakKey::ArkivId(saksnummer.clone()), true)
            .await
            .unwrap();

        assert_eq!(resultat.saksnummer, saksnummer);
        let kall = kall.lock().unwrap();
        assert!(kall.oppslag.is_empty());
        assert_eq!(kall.repo, vec![(saksnummer, true)]);
    }

    #[tokio::test]
    async fn client_reference_slaas_opp_til_saksnummer_foer_arkivkall() {
        let client_reference = Uuid::new_v4();
        let saksnummer = Saksnummer::new("2026/54321").unwrap();
        let (service, kall) = service(Some(saksnummer.clone()), true);

        service
            .handle(SakKey::ClientReference(client_reference), false)
            .await
            .unwrap();

        let kall = kall.lock().unwrap();
        assert_eq!(kall.oppslag, vec![client_reference]);
        assert_eq!(kall.repo, vec![(saksnummer, false)]);
    }

    #[tokio::test]
    async fn ukjent_client_reference_gir_feil_uten_arkivkall() {
        let (service, kall) = service(None, true);

        let resultat = service
            .handle(SakKey::ClientReference(Uuid::new_v4()), true)
            .await;

        assert!(resultat.is_err());
        assert!(kall.lock().unwrap().repo.is_empty());
    }

    #[tokio::test]
    async fn feil_fra_arkivet_propageres() {
        let (service, _) = service(None, false);

        let resultat = service
            .handle(SakKey::ArkivId(Saksnummer::new("2026/1").unwrap()), false)
            .await;

        assert_eq!(resultat.unwrap_err().to_string(), "Sak ikke funnet");
    }
}
