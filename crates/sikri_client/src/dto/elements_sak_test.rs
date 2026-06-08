#[cfg(test)]
mod tests {
    use crate::domain::ny_sak::{Arkivdel, NySak, Tilgang};
    use crate::dto::elements_sak::{DEFAULT_SAKSSTATUS, ElementsSak, JOURNALENHET};

    #[test]
    fn validate_ok() {
        let sak = ElementsSak {
            sakstittel: "Testsak".to_string(),
            ordningsverdi: "2020".to_string(),
            arkivdel: Arkivdel::Hovedkontoret.to_string(),
            journalenhet: JOURNALENHET.to_string(),
            saksbehandler: "saksbehandler".to_string(),
            saksbehandler_enhet: "saksbehandler_enhet".to_string(),
            saksstatus: DEFAULT_SAKSSTATUS.to_string(),
            tilgangskode: None,
            tilgangshjemmel: None,
            virksomhetsmappe_id: None,
        };
        assert!(sak.validate().is_ok());
    }

    #[test]
    fn validate_fail_sakstittel_empty() {
        let sak = ElementsSak {
            sakstittel: "".to_string(),
            ordningsverdi: "2020".to_string(),
            arkivdel: Arkivdel::Hovedkontoret.to_string(),
            journalenhet: JOURNALENHET.to_string(),
            saksbehandler: "saksbehandler".to_string(),
            saksbehandler_enhet: "saksbehandler_enhet".to_string(),
            saksstatus: DEFAULT_SAKSSTATUS.to_string(),
            tilgangskode: None,
            tilgangshjemmel: None,
            virksomhetsmappe_id: None,
        };
        assert!(sak.validate().is_err());
    }

    #[test]
    fn validate_fail_sakstittel_too_long() {
        let long_title = "a".repeat(257);
        let sak = ElementsSak {
            sakstittel: long_title,
            ordningsverdi: "2020".to_string(),
            arkivdel: Arkivdel::Hovedkontoret.to_string(),
            journalenhet: JOURNALENHET.to_string(),
            saksbehandler: "saksbehandler".to_string(),
            saksbehandler_enhet: "saksbehandler_enhet".to_string(),
            saksstatus: DEFAULT_SAKSSTATUS.to_string(),
            tilgangskode: None,
            tilgangshjemmel: None,
            virksomhetsmappe_id: None,
        };
        assert!(sak.validate().is_err());
    }

    #[test]
    fn validate_fail_ordningsverdi_empty() {
        let sak = ElementsSak {
            sakstittel: "Testsak".to_string(),
            ordningsverdi: "".to_string(),
            arkivdel: Arkivdel::Hovedkontoret.to_string(),
            journalenhet: JOURNALENHET.to_string(),
            saksbehandler: "saksbehandler".to_string(),
            saksbehandler_enhet: "saksbehandler_enhet".to_string(),
            saksstatus: DEFAULT_SAKSSTATUS.to_string(),
            tilgangskode: None,
            tilgangshjemmel: None,
            virksomhetsmappe_id: None,
        };
        assert!(sak.validate().is_err());
    }

    #[test]
    fn serializes_tilgang_as_top_level_sikri_fields() {
        let sak = ElementsSak::from(NySak {
            sakstittel: "[|Ola Norrmann|] - Testsak".to_string(),
            arkivdel: Arkivdel::Hovedkontoret,
            saksbehandler_id: "17804".to_string(),
            saksbehandler_enhet: "DOK".to_string(),
            ordningsverdi: "123".to_string(),
            tilgang: Some(Tilgang {
                tilgangskode: "UO".to_string(),
                tilgangshjemmel: "Offl. § 23 tredje ledd".to_string(),
            }),
            virksomhetsmappe_id: None,
        });

        let json = serde_json::to_value(sak).expect("sak serializes");

        assert_eq!(json["tilgangskode"], "UO");
        assert_eq!(json["tilgangshjemmel"], "Offl. § 23 tredje ledd");
        assert!(json.get("tilgang").is_none());
    }
}
