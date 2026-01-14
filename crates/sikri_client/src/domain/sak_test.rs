#[cfg(test)]
mod tests {
    use crate::domain::sak::SakRespons;
    use crate::dto::elements_sak_response::ElementsSakMedJournalposterResponse;

    #[test]
    fn from_response_mapping() {
        let response = ElementsSakMedJournalposterResponse {
            sakstittel: "Tittel".to_string(),
            arkivdel: Some("ARKIV".to_string()),
            journalenhet: Some("ENHET".to_string()),
            saksbehandler: Some("User".to_string()),
            saksbehandler_enhet: Some("Dept".to_string()),
            saksstatus: Some("B".to_string()),
            ordningsverdi: Some("123".to_string()),
            tilgangskode: Some("U".to_string()),
            tilgangshjemmel: Some("Paragraf".to_string()),
            virksomhetsmappe_id: Some("VM1".to_string()),
            saksid: Some(10),
            saksnr: Some("2021/1".to_string()),
            saks_url: Some("http://url".to_string()),
            kildesystem: Some("KS".to_string()),
            lukket: Some(false),
            mappetype: Some("SAK".to_string()),
            antall_journalposter: Some(0),
            journalposter: None,
        };

        let sak = SakRespons::from(response);

        assert_eq!(sak.sakstittel, "Tittel");
        assert_eq!(sak.saksid, 10);
        assert_eq!(sak.ordningsverdi, "123");
        assert!(!sak.lukket);
    }

    #[test]
    fn from_response_defaults() {
        let response = ElementsSakMedJournalposterResponse {
            sakstittel: "Tittel".to_string(),
            arkivdel: None,
            journalenhet: None,
            saksbehandler: None,
            saksbehandler_enhet: None,
            saksstatus: None,
            ordningsverdi: None,
            tilgangskode: None,
            tilgangshjemmel: None,
            virksomhetsmappe_id: None,
            saksid: None,
            saksnr: None,
            saks_url: None,
            kildesystem: None,
            lukket: None,
            mappetype: None,
            antall_journalposter: None,
            journalposter: None,
        };

        let sak = SakRespons::from(response);
        assert_eq!(sak.ordningsverdi, "");
        assert_eq!(sak.saksid, 0);
        assert!(!sak.lukket);
    }
}
