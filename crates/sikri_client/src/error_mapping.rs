use reqwest::StatusCode;

pub const IRRECOVERABLE_MARKER: &str = "sikri_recoverability=irrecoverable";
pub const RECOVERABLE_MARKER: &str = "sikri_recoverability=recoverable";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recoverability {
    Recoverable,
    Irrecoverable,
}

pub fn user_message_for_http_error(status: StatusCode, body: Option<&str>) -> String {
    if let Some(body_text) = body {
        if classify_http_error(status, Some(body_text)) == Recoverability::Irrecoverable
            && contains_missing_user_pattern(body_text)
        {
            let bruker = extract_user_from_identification_error(body_text)
                .unwrap_or_else(|| "ukjent".to_string());
            return format!(
                "Ugyldig saksbehandler/systembruker ({bruker}): brukeren finnes ikke i ePhorte (PERSON.PE_BRUKERID)."
            );
        }
    }

    format!("Sikri svarte med HTTP-feil ({status}).")
}

struct ErrorRule {
    status: Option<StatusCode>,
    body_contains_all: &'static [&'static str],
    recoverability: Recoverability,
}

const ERROR_RULES: &[ErrorRule] = &[ErrorRule {
    status: Some(StatusCode::INTERNAL_SERVER_ERROR),
    body_contains_all: &[
        "feil ved identifisering av bruker",
        "ble ikke funnet i ephorte person-tabell",
    ],
    recoverability: Recoverability::Irrecoverable,
}];

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

fn extract_user_from_identification_error(body: &str) -> Option<String> {
    let normalized = body.to_lowercase();
    let needle = "identifisering av bruker ";
    let start = normalized.find(needle)? + needle.len();
    let tail = body.get(start..)?;
    let user: String = tail
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();

    if user.is_empty() {
        None
    } else {
        Some(user)
    }
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
    fn keeps_client_errors_irrecoverable_by_default() {
        let result = classify_http_error(StatusCode::BAD_REQUEST, Some("invalid request"));
        assert_eq!(result, Recoverability::Irrecoverable);
    }

    #[test]
    fn maps_known_missing_user_error_to_friendly_message() {
        let body = "Feil ved identifisering av bruker Z12345. Person.Brukernavn Z12345 ble ikke funnet i ePhorte Person-tabell!";
        let message = user_message_for_http_error(StatusCode::INTERNAL_SERVER_ERROR, Some(body));
        assert_eq!(
            message,
            "Ugyldig saksbehandler/systembruker (Z12345): brukeren finnes ikke i ePhorte (PERSON.PE_BRUKERID)."
        );
    }
}
