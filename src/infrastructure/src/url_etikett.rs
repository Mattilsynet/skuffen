//! Trygg gjengivelse av URL-er i logg.
//!
//! En URL fra konfigurasjon kan bære legitimasjon i authority-delen. Det som
//! er nyttig for en operatør er hvor tjenesten peker, ikke hva den logger seg
//! på med.

/// Beholder `scheme://host:port` og forkaster alt annet, inkludert bruker,
/// passord, token, sti og query.
pub(crate) fn trygg_url_etikett(url: &str, standard_scheme: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or((standard_scheme, url));
    let authority = rest.split('/').next().unwrap_or(rest);
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, value)| value)
        .unwrap_or(authority);

    if host_port.is_empty() {
        return format!("{scheme}://<ukjent>");
    }

    format!("{scheme}://{host_port}")
}

#[cfg(test)]
mod tests {
    use super::trygg_url_etikett;

    #[test]
    fn bruker_og_passord_fjernes() {
        assert_eq!(
            trygg_url_etikett("https://user:secret@collector.example:4317", "https"),
            "https://collector.example:4317"
        );
    }

    #[test]
    fn token_fjernes() {
        assert_eq!(
            trygg_url_etikett("http://token@collector.internal:4317", "http"),
            "http://collector.internal:4317"
        );
    }

    #[test]
    fn sti_og_query_fjernes() {
        assert_eq!(
            trygg_url_etikett("https://collector.example/v1/traces?key=hemmelig", "https"),
            "https://collector.example"
        );
    }

    #[test]
    fn url_uten_legitimasjon_beholdes() {
        assert_eq!(
            trygg_url_etikett("http://localhost:4317", "http"),
            "http://localhost:4317"
        );
    }

    #[test]
    fn manglende_scheme_faar_standarden() {
        assert_eq!(
            trygg_url_etikett("localhost:4317", "http"),
            "http://localhost:4317"
        );
    }
}
