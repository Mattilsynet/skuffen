use reqwest::StatusCode;

pub const IRRECOVERABLE_MARKER: &str = "sikri_recoverability=irrecoverable";
pub const RECOVERABLE_MARKER: &str = "sikri_recoverability=recoverable";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recoverability {
    Recoverable,
    Irrecoverable,
}

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

struct ErrorRule {
    status: Option<StatusCode>,
    body_contains_all: &'static [&'static str],
    recoverability: Recoverability,
}

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
];

pub fn classify_http_error(status: StatusCode, body: Option<&str>) -> Recoverability {
    if let Some(body_text) = body {
        let normalized_body = body_text.to_lowercase();
        for rule in ERROR_RULES {
            if rule.status.is_some_and(|expected| expected != status) {
                continue;
            }

            if rule
                .body_contains_all
                .iter()
                .all(|needle| normalized_body.contains(needle))
            {
                return rule.recoverability;
            }
        }
    }

    if status == StatusCode::NOT_FOUND {
        return Recoverability::Irrecoverable;
    }

    if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        return Recoverability::Recoverable;
    }

    if status.is_client_error() {
        return Recoverability::Irrecoverable;
    }

    Recoverability::Recoverable
}

pub fn marker_for(recoverability: Recoverability) -> &'static str {
    match recoverability {
        Recoverability::Recoverable => RECOVERABLE_MARKER,
        Recoverability::Irrecoverable => IRRECOVERABLE_MARKER,
    }
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
    fn keeps_client_errors_irrecoverable_by_default() {
        let result = classify_http_error(StatusCode::BAD_REQUEST, Some("invalid request"));
        assert_eq!(result, Recoverability::Irrecoverable);
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
}
