use application::command::ports::command_state_port::{
    ArkivSakTilstand, ArkivSakTilstandError, ArkivSakTilstandErrorKind, ArkivSakTilstandRepository,
};
use async_trait::async_trait;
use sikri_client::Recoverability;

#[derive(Clone)]
pub struct SikriCommandStateRepository;

#[async_trait]
impl ArkivSakTilstandRepository for SikriCommandStateRepository {
    async fn hent_sak_tilstand_fra_arkivet(
        &self,
        saksnummer: &str,
    ) -> Result<ArkivSakTilstand, ArkivSakTilstandError> {
        match sikri_client::hent_sak(saksnummer, "SKUFFEN", false).await {
            Ok(sak) => {
                let avsluttet = sak.lukket
                    || sak
                        .saksstatus
                        .as_deref()
                        .and_then(|status| status.chars().next())
                        == Some('A');
                Ok(ArkivSakTilstand { avsluttet })
            }
            Err(err) => {
                if let Some(req_err) = err.downcast_ref::<reqwest::Error>() {
                    let status = req_err.status();
                    let kind = map_status_to_kind(status);
                    let message = map_status_to_safe_code(status);
                    return Err(ArkivSakTilstandError::new(kind, message));
                }

                Err(ArkivSakTilstandError::new(
                    ArkivSakTilstandErrorKind::Recoverable,
                    "sikri_upstream_unavailable",
                ))
            }
        }
    }
}

fn map_status_to_kind(status: Option<reqwest::StatusCode>) -> ArkivSakTilstandErrorKind {
    match status {
        Some(status) => match sikri_client::classify_http_error(status, None) {
            Recoverability::Recoverable => ArkivSakTilstandErrorKind::Recoverable,
            Recoverability::Irrecoverable => ArkivSakTilstandErrorKind::Irrecoverable,
        },
        None => ArkivSakTilstandErrorKind::Recoverable,
    }
}

fn map_status_to_safe_code(status: Option<reqwest::StatusCode>) -> &'static str {
    match status {
        Some(status) => sikri_client::safe_detail_for_http_error(status, None),
        None => "sikri_upstream_unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_404_to_irrecoverable_and_safe_code() {
        let kind = map_status_to_kind(Some(reqwest::StatusCode::NOT_FOUND));
        let code = map_status_to_safe_code(Some(reqwest::StatusCode::NOT_FOUND));
        assert_eq!(kind, ArkivSakTilstandErrorKind::Irrecoverable);
        assert_eq!(code, "sikri_resource_not_found");
    }

    #[test]
    fn maps_503_to_recoverable_and_safe_code() {
        let kind = map_status_to_kind(Some(reqwest::StatusCode::SERVICE_UNAVAILABLE));
        let code = map_status_to_safe_code(Some(reqwest::StatusCode::SERVICE_UNAVAILABLE));
        assert_eq!(kind, ArkivSakTilstandErrorKind::Recoverable);
        assert_eq!(code, "sikri_upstream_error");
    }

    #[test]
    fn maps_429_to_recoverable_and_safe_code() {
        let kind = map_status_to_kind(Some(reqwest::StatusCode::TOO_MANY_REQUESTS));
        let code = map_status_to_safe_code(Some(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(kind, ArkivSakTilstandErrorKind::Recoverable);
        assert_eq!(code, "sikri_rate_limited");
    }

    #[test]
    fn maps_400_to_irrecoverable_and_safe_code() {
        let kind = map_status_to_kind(Some(reqwest::StatusCode::BAD_REQUEST));
        let code = map_status_to_safe_code(Some(reqwest::StatusCode::BAD_REQUEST));
        assert_eq!(kind, ArkivSakTilstandErrorKind::Irrecoverable);
        assert_eq!(code, "sikri_invalid_request");
    }

    #[test]
    fn maps_none_status_to_recoverable_and_safe_code() {
        let kind = map_status_to_kind(None);
        let code = map_status_to_safe_code(None);
        assert_eq!(kind, ArkivSakTilstandErrorKind::Recoverable);
        assert_eq!(code, "sikri_upstream_unavailable");
    }

    #[test]
    fn safe_codes_never_contain_sensitive_data() {
        let statuses = vec![
            reqwest::StatusCode::NOT_FOUND,
            reqwest::StatusCode::BAD_REQUEST,
            reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            reqwest::StatusCode::TOO_MANY_REQUESTS,
        ];
        for status in statuses {
            let code = map_status_to_safe_code(Some(status));
            assert!(!code.contains("http"), "Code for {} contains http", status);
            assert!(!code.contains("http"), "Code for {} contains https", status);
            assert!(
                !code.contains("/"),
                "Code for {} contains path separator",
                status
            );
        }
    }

    #[test]
    fn safe_codes_are_stable_identifier_strings() {
        let code = map_status_to_safe_code(Some(reqwest::StatusCode::NOT_FOUND));
        assert!(code.starts_with("sikri_"));
        assert!(code.chars().all(|c| c.is_alphanumeric() || c == '_'));
    }
}
