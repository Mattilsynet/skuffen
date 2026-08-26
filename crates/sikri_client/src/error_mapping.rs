use reqwest::StatusCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recoverability {
    Recoverable,
    Irrecoverable,
}

impl Recoverability {
    pub fn as_str(self) -> &'static str {
        match self {
            Recoverability::Recoverable => "recoverable",
            Recoverability::Irrecoverable => "irrecoverable",
        }
    }
}

/// Feilen slik `sikri_client` klassifiserer den.
///
/// `kode` er stabil og greppbar; `melding` er trygg ved konstruksjon og går
/// videre til klienten uendret. Ingen av dem bærer bruker-id, tilgangskode
/// eller URL — det låses av testene nederst i denne filen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SikriFeil {
    pub recoverability: Recoverability,
    pub kode: &'static str,
    pub melding: String,
}

impl SikriFeil {
    pub fn new(
        recoverability: Recoverability,
        kode: &'static str,
        melding: impl Into<String>,
    ) -> Self {
        Self {
            recoverability,
            kode,
            melding: melding.into(),
        }
    }

    pub fn recoverable(kode: &'static str, melding: impl Into<String>) -> Self {
        Self::new(Recoverability::Recoverable, kode, melding)
    }

    pub fn irrecoverable(kode: &'static str, melding: impl Into<String>) -> Self {
        Self::new(Recoverability::Irrecoverable, kode, melding)
    }

    /// Klassifiserer et HTTP-feilsvar der bodyen er lest.
    pub fn fra_http(status: StatusCode, body: Option<&str>) -> Self {
        Self::new(
            classify_http_error(status, body),
            safe_detail_for_http_error(status, body),
            user_message_for_http_error(status, body),
        )
    }

    /// Sikri er ikke nåbar. Alltid recoverable — det er ingenting Skuffen kan
    /// rette ved å gi opp.
    pub fn utilgjengelig() -> Self {
        Self::recoverable(
            "sikri_upstream_unavailable",
            "Sikri/Elements er midlertidig utilgjengelig. Prøv igjen senere.",
        )
    }

    /// Credentials kunne ikke hentes fra Secret Manager.
    pub fn secret_utilgjengelig() -> Self {
        Self::recoverable(
            "sikri_secret_unavailable",
            "Sikri/Elements er midlertidig utilgjengelig. Prøv igjen senere.",
        )
    }

    /// Sikri svarte 2xx med en form vi ikke kjenner igjen. Recoverable fordi
    /// et formatavvik hos leverandøren ikke er noe Skuffen kan rette.
    pub fn uparsbart_svar() -> Self {
        Self::recoverable(
            "sikri_response_unparsable",
            "Uventet svar fra Sikri/Elements. Prøv igjen senere.",
        )
    }

    pub fn er_recoverable(&self) -> bool {
        self.recoverability == Recoverability::Recoverable
    }
}

impl std::fmt::Display for SikriFeil {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kode, self.melding)
    }
}

impl std::error::Error for SikriFeil {}

pub fn user_message_for_http_error(status: StatusCode, body: Option<&str>) -> String {
    if let Some(body_text) = body
        && contains_upstream_bad_gateway_pattern(body_text)
    {
        return "Sikri/Elements er midlertidig utilgjengelig. Prøv igjen senere.".to_string();
    }

    if let Some(body_text) = body
        && classify_http_error(status, Some(body_text)) == Recoverability::Irrecoverable
        && contains_missing_user_pattern(body_text)
    {
        return "Ugyldig saksbehandler/systembruker: brukeren finnes ikke i ePhorte.".to_string();
    }

    if status == StatusCode::TOO_MANY_REQUESTS {
        return "Sikri/Elements avviser midlertidig for mange forespørsler. Prøv igjen senere."
            .to_string();
    }

    if status.is_server_error() {
        return "Sikri/Elements er midlertidig utilgjengelig. Prøv igjen senere.".to_string();
    }

    "Sikri/Elements avviste forespørselen.".to_string()
}

pub fn safe_detail_for_http_error(status: StatusCode, body: Option<&str>) -> &'static str {
    if let Some(body_text) = body
        && contains_upstream_bad_gateway_pattern(body_text)
    {
        return "sikri_upstream_unavailable";
    }

    if let Some(body_text) = body
        && classify_http_error(status, Some(body_text)) == Recoverability::Irrecoverable
        && contains_missing_user_pattern(body_text)
    {
        return "sikri_unknown_user";
    }

    if let Some(body_text) = body
        && body_text
            .to_lowercase()
            .contains("ny journalpost har dokument-filer som mangler innhold")
    {
        return "sikri_missing_document_content";
    }

    if let Some(body_text) = body
        && access_control_failure_is_irrecoverable(status)
        && contains_access_control_pattern(body_text)
    {
        return "sikri_access_control_rejected";
    }

    if let Some(body_text) = body
        && validation_failure_is_irrecoverable(status)
        && contains_validation_pattern(body_text)
    {
        return "sikri_validation_failed";
    }

    if status == StatusCode::NOT_FOUND {
        return "sikri_resource_not_found";
    }

    if status == StatusCode::TOO_MANY_REQUESTS {
        return "sikri_rate_limited";
    }

    if status.is_server_error() {
        return "sikri_upstream_error";
    }

    if status.is_client_error() {
        return "sikri_invalid_request";
    }

    "sikri_unknown_error"
}

/// Hver kode `safe_detail_for_http_error` og `SikriFeil` kan produsere.
///
/// Adapterne i `infrastructure` oversetter disse til klientvendte feilkoder,
/// og har en test som går gjennom listen. Legger du til en kode uten å legge
/// den inn her, fanges det ikke — legger du den inn her uten å mappe den,
/// feiler adaptertesten. Det er den veien vi vil ha det.
pub const ALLE_SIKRI_KODER: &[&str] = &[
    "sikri_unknown_user",
    "sikri_access_control_rejected",
    "sikri_validation_failed",
    "sikri_missing_document_content",
    "sikri_resource_not_found",
    "sikri_rate_limited",
    "sikri_upstream_error",
    "sikri_upstream_unavailable",
    "sikri_invalid_request",
    "sikri_unknown_error",
    "sikri_secret_unavailable",
    "sikri_response_unparsable",
    "sikri_request_validation_failed",
];

struct ErrorRule {
    status: Option<StatusCode>,
    body_contains_all: &'static [&'static str],
    recoverability: Recoverability,
}

/// Regelsettet leses ovenfra og ned, og første treff vinner.
///
/// Body-reglene ligger først. Statusreglene til slutt stiller ingen krav til
/// bodyen og treffer derfor alt med den statuskoden — lagt først ville de
/// skygget for body-reglene over.
const ERROR_RULES: &[ErrorRule] = &[
    ErrorRule {
        status: Some(StatusCode::INTERNAL_SERVER_ERROR),
        body_contains_all: &["feil ved identifisering av bruker", "502", "bad gateway"],
        recoverability: Recoverability::Recoverable,
    },
    ErrorRule {
        status: Some(StatusCode::INTERNAL_SERVER_ERROR),
        body_contains_all: &[
            "feil ved identifisering av bruker",
            "ble ikke funnet i ephorte person-tabell",
        ],
        recoverability: Recoverability::Irrecoverable,
    },
    ErrorRule {
        status: Some(StatusCode::INTERNAL_SERVER_ERROR),
        body_contains_all: &["ny journalpost har dokument-filer som mangler innhold"],
        recoverability: Recoverability::Irrecoverable,
    },
    ErrorRule {
        status: Some(StatusCode::BAD_REQUEST),
        body_contains_all: &["tilgangskode"],
        recoverability: Recoverability::Irrecoverable,
    },
    ErrorRule {
        status: Some(StatusCode::INTERNAL_SERVER_ERROR),
        body_contains_all: &["tilgangskode"],
        recoverability: Recoverability::Irrecoverable,
    },
    ErrorRule {
        status: Some(StatusCode::BAD_REQUEST),
        body_contains_all: &["tilgangshjemmel"],
        recoverability: Recoverability::Irrecoverable,
    },
    ErrorRule {
        status: Some(StatusCode::INTERNAL_SERVER_ERROR),
        body_contains_all: &["tilgangshjemmel"],
        recoverability: Recoverability::Irrecoverable,
    },
    ErrorRule {
        status: Some(StatusCode::BAD_REQUEST),
        body_contains_all: &["mangler tilgang"],
        recoverability: Recoverability::Irrecoverable,
    },
    ErrorRule {
        status: Some(StatusCode::INTERNAL_SERVER_ERROR),
        body_contains_all: &["mangler tilgang"],
        recoverability: Recoverability::Irrecoverable,
    },
    ErrorRule {
        status: Some(StatusCode::BAD_REQUEST),
        body_contains_all: &["ikke har rettighet"],
        recoverability: Recoverability::Irrecoverable,
    },
    ErrorRule {
        status: Some(StatusCode::INTERNAL_SERVER_ERROR),
        body_contains_all: &["ikke har rettighet"],
        recoverability: Recoverability::Irrecoverable,
    },
    ErrorRule {
        status: Some(StatusCode::BAD_REQUEST),
        body_contains_all: &["validering", "feil"],
        recoverability: Recoverability::Irrecoverable,
    },
    ErrorRule {
        status: Some(StatusCode::INTERNAL_SERVER_ERROR),
        body_contains_all: &["validering", "feil"],
        recoverability: Recoverability::Irrecoverable,
    },
    // --- Statusregler. Ingen body-krav; må ligge sist. ---
    ErrorRule {
        status: Some(StatusCode::NOT_FOUND),
        body_contains_all: &[],
        recoverability: Recoverability::Irrecoverable,
    },
    // Sikri autentiseres med brukernavn/passord uten token-refresh. Et rotert
    // passord eller en hikke i Secret Manager skal gi retry, ikke terminere
    // hver operasjon som er underveis — `feilet` kan ikke trekkes tilbake.
    ErrorRule {
        status: Some(StatusCode::UNAUTHORIZED),
        body_contains_all: &[],
        recoverability: Recoverability::Recoverable,
    },
    ErrorRule {
        status: Some(StatusCode::FORBIDDEN),
        body_contains_all: &[],
        recoverability: Recoverability::Recoverable,
    },
    ErrorRule {
        status: Some(StatusCode::TOO_MANY_REQUESTS),
        body_contains_all: &[],
        recoverability: Recoverability::Recoverable,
    },
];

fn rule_matches(rule: &ErrorRule, status: StatusCode, normalized_body: Option<&str>) -> bool {
    if rule.status.is_some_and(|expected| expected != status) {
        return false;
    }

    if rule.body_contains_all.is_empty() {
        return true;
    }

    let Some(body) = normalized_body else {
        return false;
    };

    rule.body_contains_all
        .iter()
        .all(|needle| body.contains(needle))
}

/// Terminal feil krever positivt treff i regelsettet.
///
/// Bunnen er `Recoverable`: en ukjent feil retryes til noen legger inn en
/// regel for den. Å retrye en ekte klientfeil er billig og reversibelt, mens
/// `feilet` er monotont og publiseres til klienten uten vei tilbake (SKU-0016
/// R8). Kodene i `siste_detalj` gjør de ukartlagte tilfellene synlige.
pub fn classify_http_error(status: StatusCode, body: Option<&str>) -> Recoverability {
    let normalized_body = body.map(|body_text| body_text.to_lowercase());

    for rule in ERROR_RULES {
        if rule_matches(rule, status, normalized_body.as_deref()) {
            return rule.recoverability;
        }
    }

    Recoverability::Recoverable
}

fn contains_missing_user_pattern(body: &str) -> bool {
    let normalized = body.to_lowercase();
    normalized.contains("feil ved identifisering av bruker")
        && normalized.contains("ble ikke funnet i ephorte person-tabell")
}

fn contains_upstream_bad_gateway_pattern(body: &str) -> bool {
    let normalized = body.to_lowercase();
    normalized.contains("feil ved identifisering av bruker")
        && normalized.contains("502")
        && normalized.contains("bad gateway")
}

fn contains_access_control_pattern(body: &str) -> bool {
    let normalized = body.to_lowercase();
    normalized.contains("tilgangskode")
        || normalized.contains("tilgangshjemmel")
        || normalized.contains("mangler tilgang")
        || normalized.contains("ikke har rettighet")
}

fn access_control_failure_is_irrecoverable(status: StatusCode) -> bool {
    status == StatusCode::BAD_REQUEST || status == StatusCode::INTERNAL_SERVER_ERROR
}

fn contains_validation_pattern(body: &str) -> bool {
    let normalized = body.to_lowercase();
    normalized.contains("validering") && normalized.contains("feil")
}

fn validation_failure_is_irrecoverable(status: StatusCode) -> bool {
    status == StatusCode::BAD_REQUEST || status == StatusCode::INTERNAL_SERVER_ERROR
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_known_internal_server_error_as_irrecoverable() {
        let body = "Feil ved identifisering av bruker Z12345. Person.Brukernavn Z12345 ble ikke funnet i ePhorte Person-tabell!";
        let result = classify_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        assert_eq!(result, Recoverability::Irrecoverable);
    }

    #[test]
    fn keeps_unknown_internal_server_error_recoverable() {
        let result = classify_http_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            Some("temporary backend issue"),
        );
        assert_eq!(result, Recoverability::Recoverable);
    }

    #[test]
    fn marks_missing_document_content_error_as_irrecoverable() {
        let body = "Ny journalpost har dokument-filer som mangler innhold";
        let result = classify_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        assert_eq!(result, Recoverability::Irrecoverable);
    }

    #[test]
    fn marks_identification_error_with_upstream_bad_gateway_as_recoverable() {
        let body = "Feil ved identifisering av bruker SikriArkivApi. Detaljer: The remote server returned an unexpected response: (502) Bad Gateway.";
        let result = classify_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        assert_eq!(result, Recoverability::Recoverable);
    }

    #[test]
    fn ukjente_klientfeil_er_recoverable() {
        // Terminal feil krever positivt treff. En ukjent 4xx retryes til noen
        // legger inn en regel for den.
        for status in [
            StatusCode::BAD_REQUEST,
            StatusCode::CONFLICT,
            StatusCode::UNPROCESSABLE_ENTITY,
        ] {
            assert_eq!(
                classify_http_error(status, Some("invalid request")),
                Recoverability::Recoverable,
                "{status} uten kjent body skal være recoverable"
            );
            assert_eq!(
                classify_http_error(status, None),
                Recoverability::Recoverable,
                "{status} uten body skal være recoverable"
            );
        }
    }

    #[test]
    fn autentiseringsfeil_er_recoverable() {
        // Det viktigste enkelttilfellet: et rotert passord skal ikke
        // terminere hver operasjon som er underveis.
        for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
            assert_eq!(
                classify_http_error(status, None),
                Recoverability::Recoverable
            );
            assert_eq!(
                classify_http_error(status, Some("access denied")),
                Recoverability::Recoverable
            );
        }
    }

    #[test]
    fn not_found_er_irrecoverable_uten_body() {
        // Valideringen er avhengig av dette: et saksnummer som ikke finnes
        // skal avvises, ikke retryes i en varm løkke mot arkivet.
        //
        // Merk at AvventJournalfort også poller mot 404-veien. Ser vi at
        // polling begynner å terminere, er det denne regelen som skal
        // revurderes — ikke bunnen i classify_http_error.
        assert_eq!(
            classify_http_error(StatusCode::NOT_FOUND, None),
            Recoverability::Irrecoverable
        );
        assert_eq!(
            classify_http_error(StatusCode::NOT_FOUND, Some("finnes ikke")),
            Recoverability::Irrecoverable
        );
    }

    #[test]
    fn body_regler_gaar_foran_statusregler() {
        // 400 er recoverable som bunn, men et positivt treff på tilgangskode
        // skal fortsatt terminere. Rekkefølgen i ERROR_RULES er det som
        // holder dette oppe.
        assert_eq!(
            classify_http_error(StatusCode::BAD_REQUEST, Some("Ugyldig tilgangskode UO")),
            Recoverability::Irrecoverable
        );
        assert_eq!(
            classify_http_error(StatusCode::BAD_REQUEST, Some("tilgangshjemmel mangler")),
            Recoverability::Irrecoverable
        );
    }

    #[test]
    fn serverfeil_er_recoverable() {
        for status in [
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::GATEWAY_TIMEOUT,
        ] {
            assert_eq!(
                classify_http_error(status, None),
                Recoverability::Recoverable
            );
        }
    }

    #[test]
    fn maps_known_missing_user_error_to_friendly_message_without_user_id() {
        let body = "Feil ved identifisering av bruker Z12345. Person.Brukernavn Z12345 ble ikke funnet i ePhorte Person-tabell!";
        let message = user_message_for_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        assert_eq!(
            message,
            "Ugyldig saksbehandler/systembruker: brukeren finnes ikke i ePhorte."
        );
        assert!(!message.contains("Z12345"));
    }

    #[test]
    fn maps_upstream_bad_gateway_to_friendly_message() {
        let body = "Feil ved identifisering av bruker SikriArkivApi. Detaljer: The remote server returned an unexpected response: (502) Bad Gateway.";
        let message = user_message_for_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        assert_eq!(
            message,
            "Sikri/Elements er midlertidig utilgjengelig. Prøv igjen senere."
        );
    }

    #[test]
    fn safe_detail_returns_stable_code_without_user_id() {
        let body = "Feil ved identifisering av bruker Z12345. Person.Brukernavn Z12345 ble ikke funnet i ePhorte Person-tabell!";
        let detail = safe_detail_for_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        assert_eq!(detail, "sikri_unknown_user");
        assert!(!detail.contains("Z12345"));
        assert!(!detail.contains("bruker"));
    }

    #[test]
    fn safe_detail_returns_stable_code_without_upstream_text_or_url() {
        let body = "temporary backend issue at https://internal.example.invalid/api";
        let detail = safe_detail_for_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        assert_eq!(detail, "sikri_upstream_error");
        assert!(!detail.contains("http"));
        assert!(!detail.contains("temporary"));
    }

    #[test]
    fn safe_detail_maps_known_status_codes() {
        assert_eq!(
            safe_detail_for_http_error(StatusCode::NOT_FOUND, None),
            "sikri_resource_not_found"
        );
        assert_eq!(
            safe_detail_for_http_error(StatusCode::TOO_MANY_REQUESTS, None),
            "sikri_rate_limited"
        );
        assert_eq!(
            safe_detail_for_http_error(StatusCode::BAD_REQUEST, None),
            "sikri_invalid_request"
        );
    }

    #[test]
    fn maps_tilgangskode_errors_to_safe_access_code() {
        let body = "Tilgangskode UO er ugyldig for denne saken og bruker Z12345";
        let result = classify_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        let detail = safe_detail_for_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));

        assert_eq!(result, Recoverability::Irrecoverable);
        assert_eq!(detail, "sikri_access_control_rejected");
        assert!(!detail.contains("UO"));
        assert!(!detail.contains("Z12345"));
        assert!(!detail.contains("Tilgangskode"));
    }

    #[test]
    fn maps_tilgangshjemmel_errors_to_safe_access_code() {
        let body = "Mangler tilgangshjemmel for skjermet operasjon";
        let result = classify_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        let detail = safe_detail_for_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));

        assert_eq!(result, Recoverability::Irrecoverable);
        assert_eq!(detail, "sikri_access_control_rejected");
        assert!(!detail.contains("tilgangshjemmel"));
    }

    #[test]
    fn maps_validation_errors_to_safe_validation_code() {
        let body = "Validering feilet for felt med verdi som ikke skal logges";
        let result = classify_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        let detail = safe_detail_for_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));

        assert_eq!(result, Recoverability::Irrecoverable);
        assert_eq!(detail, "sikri_validation_failed");
        assert!(!detail.contains("felt"));
    }

    #[test]
    fn keeps_validation_text_on_service_unavailable_recoverable() {
        let body = "Validering feilet fordi ekstern valideringstjeneste er utilgjengelig";
        let result = classify_http_error(StatusCode::SERVICE_UNAVAILABLE, Some(body));
        let detail = safe_detail_for_http_error(StatusCode::SERVICE_UNAVAILABLE, Some(body));

        assert_eq!(result, Recoverability::Recoverable);
        assert_eq!(detail, "sikri_upstream_error");
    }

    #[test]
    fn keeps_validation_text_on_rate_limit_recoverable() {
        let body = "Validering feilet fordi tjenesten er rate limited";
        let result = classify_http_error(StatusCode::TOO_MANY_REQUESTS, Some(body));
        let detail = safe_detail_for_http_error(StatusCode::TOO_MANY_REQUESTS, Some(body));

        assert_eq!(result, Recoverability::Recoverable);
        assert_eq!(detail, "sikri_rate_limited");
    }

    #[test]
    fn keeps_access_text_on_service_unavailable_recoverable() {
        let body = "Tilgangskode kunne ikke valideres fordi ekstern tjeneste er utilgjengelig";
        let result = classify_http_error(StatusCode::SERVICE_UNAVAILABLE, Some(body));
        let detail = safe_detail_for_http_error(StatusCode::SERVICE_UNAVAILABLE, Some(body));

        assert_eq!(result, Recoverability::Recoverable);
        assert_eq!(detail, "sikri_upstream_error");
    }

    #[test]
    fn keeps_access_text_on_rate_limit_recoverable() {
        let body = "Mangler tilgang kunne ikke valideres fordi tjenesten er rate limited";
        let result = classify_http_error(StatusCode::TOO_MANY_REQUESTS, Some(body));
        let detail = safe_detail_for_http_error(StatusCode::TOO_MANY_REQUESTS, Some(body));

        assert_eq!(result, Recoverability::Recoverable);
        assert_eq!(detail, "sikri_rate_limited");
    }

    #[test]
    fn maps_missing_access_errors_to_safe_access_code() {
        let body = "Mangler tilgang til skjermet sak for bruker Z12345";
        let result = classify_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        let detail = safe_detail_for_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));

        assert_eq!(result, Recoverability::Irrecoverable);
        assert_eq!(detail, "sikri_access_control_rejected");
        assert!(!detail.contains("Z12345"));
        assert!(!detail.contains("skjermet"));
    }

    #[test]
    fn maps_missing_permission_errors_to_safe_access_code() {
        let body = "Brukeren ikke har rettighet til tilgangskode XX";
        let result = classify_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        let detail = safe_detail_for_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));

        assert_eq!(result, Recoverability::Irrecoverable);
        assert_eq!(detail, "sikri_access_control_rejected");
        assert!(!detail.contains("XX"));
    }

    #[test]
    fn exposes_recoverability_as_safe_label() {
        assert_eq!(Recoverability::Recoverable.as_str(), "recoverable");
        assert_eq!(Recoverability::Irrecoverable.as_str(), "irrecoverable");
    }

    #[test]
    fn alle_produserbare_koder_staar_i_listen() {
        // Listen er kontrakten adapterne oversetter fra. Produserer
        // klassifiseringen en kode som ikke står der, er den usynlig for
        // dekningstestene i infrastructure.
        let statuser = [
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::NOT_FOUND,
            StatusCode::CONFLICT,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_GATEWAY,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::MOVED_PERMANENTLY,
        ];
        let bodyer = [
            None,
            Some("ukjent feil"),
            Some("Feil ved identifisering av bruker Z1. ble ikke funnet i ePhorte Person-tabell!"),
            Some("Feil ved identifisering av bruker X. (502) Bad Gateway"),
            Some("Ny journalpost har dokument-filer som mangler innhold"),
            Some("Ugyldig tilgangskode"),
            Some("Mangler tilgangshjemmel"),
            Some("Brukeren ikke har rettighet"),
            Some("Validering feilet"),
        ];

        for status in statuser {
            for body in bodyer {
                let kode = safe_detail_for_http_error(status, body);
                assert!(
                    ALLE_SIKRI_KODER.contains(&kode),
                    "{kode} (fra {status}) mangler i ALLE_SIKRI_KODER"
                );
            }
        }

        for feil in [
            SikriFeil::utilgjengelig(),
            SikriFeil::secret_utilgjengelig(),
            SikriFeil::uparsbart_svar(),
        ] {
            assert!(
                ALLE_SIKRI_KODER.contains(&feil.kode),
                "{} mangler i ALLE_SIKRI_KODER",
                feil.kode
            );
        }
    }

    #[test]
    fn sikri_feil_baerer_klassifisering_kode_og_melding() {
        let body = "Feil ved identifisering av bruker Z12345. Person.Brukernavn Z12345 ble ikke funnet i ePhorte Person-tabell!";
        let feil = SikriFeil::fra_http(StatusCode::INTERNAL_SERVER_ERROR, Some(body));

        assert_eq!(feil.recoverability, Recoverability::Irrecoverable);
        assert!(!feil.er_recoverable());
        assert_eq!(feil.kode, "sikri_unknown_user");
        assert_eq!(
            feil.melding,
            "Ugyldig saksbehandler/systembruker: brukeren finnes ikke i ePhorte."
        );
    }

    #[test]
    fn sikri_feil_lekker_ikke_bruker_id_tilgangskode_eller_url() {
        let bodyer = [
            "Feil ved identifisering av bruker Z12345. Person.Brukernavn Z12345 ble ikke funnet i ePhorte Person-tabell!",
            "Tilgangskode UO er ugyldig for denne saken og bruker Z12345",
            "Mangler tilgang til skjermet sak for bruker Z12345",
            "temporary backend issue at https://internal.example.invalid/api",
        ];

        for body in bodyer {
            let feil = SikriFeil::fra_http(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
            for lekkasje in ["Z12345", "UO", "http", "internal.example.invalid"] {
                assert!(
                    !feil.kode.contains(lekkasje),
                    "kode {} lekker {lekkasje}",
                    feil.kode
                );
                assert!(
                    !feil.melding.contains(lekkasje),
                    "melding {} lekker {lekkasje}",
                    feil.melding
                );
            }
        }
    }

    #[test]
    fn ikke_http_feil_er_klassifisert_eksplisitt() {
        // Ingenting skal falle gjennom til en implisitt default. Alle tre er
        // recoverable: verken en utilgjengelig Sikri, en hikke i Secret
        // Manager eller et formatavvik hos leverandøren er noe Skuffen kan
        // rette ved å gi opp.
        for feil in [
            SikriFeil::utilgjengelig(),
            SikriFeil::secret_utilgjengelig(),
            SikriFeil::uparsbart_svar(),
        ] {
            assert!(feil.er_recoverable(), "{} skal være recoverable", feil.kode);
            assert!(feil.kode.starts_with("sikri_"));
            assert!(!feil.melding.is_empty());
        }

        assert_eq!(
            SikriFeil::utilgjengelig().kode,
            "sikri_upstream_unavailable"
        );
        assert_eq!(
            SikriFeil::secret_utilgjengelig().kode,
            "sikri_secret_unavailable"
        );
        assert_eq!(
            SikriFeil::uparsbart_svar().kode,
            "sikri_response_unparsable"
        );
    }
}
