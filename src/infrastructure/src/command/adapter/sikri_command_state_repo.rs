use application::command::ports::command_state_port::{
    ArkivSakTilstand, ArkivSakTilstandError, ArkivSakTilstandErrorKind, ArkivSakTilstandRepository,
};
use async_trait::async_trait;
use domain::eksekvering::typer::StatusErrorCode;
use sikri_client::{Recoverability, SikriFeil};

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
            // Klassifiseringen er allerede gjort, der bodyen fantes.
            Err(feil) => Err(fra_sikri(feil)),
        }
    }
}

/// `SikriFeil` bærer klassifisering, stabil kode og en trygg brukertekst.
/// Her legges kun den klientvendte feilkoden på.
fn fra_sikri(feil: SikriFeil) -> ArkivSakTilstandError {
    let kind = match feil.recoverability {
        Recoverability::Recoverable => ArkivSakTilstandErrorKind::Recoverable,
        Recoverability::Irrecoverable => ArkivSakTilstandErrorKind::Irrecoverable,
    };
    ArkivSakTilstandError::new(
        kind,
        feil.kode,
        feil.melding,
        error_code_for(feil.kode).unwrap_or(StatusErrorCode::ProcessingFailed),
    )
}

fn error_code_for(kode: &str) -> Option<StatusErrorCode> {
    let error_code = match kode {
        "sikri_unknown_user"
        | "sikri_access_control_rejected"
        | "sikri_validation_failed"
        | "sikri_missing_document_content"
        | "sikri_invalid_request"
        | "sikri_request_validation_failed" => StatusErrorCode::InvalidRequest,
        "sikri_resource_not_found" => StatusErrorCode::NotFound,
        "sikri_rate_limited"
        | "sikri_upstream_unavailable"
        | "sikri_upstream_error"
        | "sikri_secret_unavailable" => StatusErrorCode::TemporaryUnavailable,
        "sikri_response_unparsable" | "sikri_unknown_error" => StatusErrorCode::ProcessingFailed,
        _ => return None,
    };
    Some(error_code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    /// Testene går gjennom `SikriFeil::fra_http`, altså samme vei som
    /// produksjonskoden. Den forrige utgaven kalte mappingfunksjonen direkte
    /// og var grønn mens kallveien var brutt.
    fn klassifiser(status: StatusCode, body: Option<&str>) -> ArkivSakTilstandError {
        fra_sikri(SikriFeil::fra_http(status, body))
    }

    #[test]
    fn alle_sikri_koder_har_en_klientvendt_feilkode() {
        // En ny kode uten oppføring faller til ProcessingFailed i drift.
        // Denne testen tvinger noen til å ta stilling til hva klienten skal
        // se før koden rekker å nå dit.
        for kode in sikri_client::ALLE_SIKRI_KODER {
            assert!(
                error_code_for(kode).is_some(),
                "{kode} mangler oversettelse til en klientvendt feilkode"
            );
        }
    }

    #[test]
    fn ukjent_saksnummer_avvises_terminalt() {
        let feil = klassifiser(StatusCode::NOT_FOUND, Some("Fant ikke arkivsak"));

        assert_eq!(feil.kind, ArkivSakTilstandErrorKind::Irrecoverable);
        assert_eq!(feil.kode, "sikri_resource_not_found");
        assert_eq!(feil.error_code, StatusErrorCode::NotFound);
    }

    #[test]
    fn ukjent_klientfeil_retryes() {
        // Bunnen i klassifiseringen: terminal feil krever positivt treff.
        for status in [
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::CONFLICT,
        ] {
            let feil = klassifiser(status, None);
            assert_eq!(
                feil.kind,
                ArkivSakTilstandErrorKind::Recoverable,
                "{status} skal retryes"
            );
        }
    }

    #[test]
    fn arkivet_nede_retryes() {
        for status in [
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            let feil = klassifiser(status, None);
            assert_eq!(feil.kind, ArkivSakTilstandErrorKind::Recoverable);
            assert_eq!(feil.error_code, StatusErrorCode::TemporaryUnavailable);
        }
    }

    #[test]
    fn body_baserte_regler_naar_frem_i_valideringen() {
        // Den gamle koden kalte classify_http_error(status, None) — uten
        // body — så ingen av body-reglene kunne slå til her i det hele tatt.
        let body = "Feil ved identifisering av bruker Z12345. Person.Brukernavn Z12345 ble ikke funnet i ePhorte Person-tabell!";
        let feil = klassifiser(StatusCode::INTERNAL_SERVER_ERROR, Some(body));

        assert_eq!(feil.kind, ArkivSakTilstandErrorKind::Irrecoverable);
        assert_eq!(feil.kode, "sikri_unknown_user");
        assert_eq!(feil.error_code, StatusErrorCode::InvalidRequest);
    }

    #[test]
    fn transportfeil_retryes() {
        let feil = fra_sikri(SikriFeil::utilgjengelig());

        assert_eq!(feil.kind, ArkivSakTilstandErrorKind::Recoverable);
        assert_eq!(feil.error_code, StatusErrorCode::TemporaryUnavailable);
    }

    #[test]
    fn koder_er_stabile_identifikatorer_uten_sensitiv_data() {
        let bodyer = [
            Some("Tilgangskode UO er ugyldig for bruker Z12345"),
            Some("temporary backend issue at https://internal.example.invalid/api"),
            None,
        ];

        for body in bodyer {
            let feil = klassifiser(StatusCode::INTERNAL_SERVER_ERROR, body);
            assert!(feil.kode.starts_with("sikri_"));
            assert!(feil.kode.chars().all(|c| c.is_alphanumeric() || c == '_'));
            for lekkasje in ["Z12345", "UO", "http", "/"] {
                assert!(!feil.kode.contains(lekkasje));
            }
        }
    }
}
