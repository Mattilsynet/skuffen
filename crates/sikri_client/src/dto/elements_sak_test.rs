#[cfg(test)]
mod tests {
    use crate::dto::elements_sak::ElementsSak;

    #[test]
    fn validate_ok() {
        let sak = ElementsSak {
            sakstittel: "Testsak".to_string(),
            ordningsverdi: "2020".to_string(),
            arkivdel: None,
            journalenhet: None,
            saksbehandler: None,
            saksbehandler_enhet: None,
            saksstatus: None,
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
            arkivdel: None,
            journalenhet: None,
            saksbehandler: None,
            saksbehandler_enhet: None,
            saksstatus: None,
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
            arkivdel: None,
            journalenhet: None,
            saksbehandler: None,
            saksbehandler_enhet: None,
            saksstatus: None,
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
            arkivdel: None,
            journalenhet: None,
            saksbehandler: None,
            saksbehandler_enhet: None,
            saksstatus: None,
            tilgangskode: None,
            tilgangshjemmel: None,
            virksomhetsmappe_id: None,
        };
        assert!(sak.validate().is_err());
    }
}
