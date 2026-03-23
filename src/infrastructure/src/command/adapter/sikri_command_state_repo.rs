use application::command::ports::command_state_port::{
    ArkivSakTilstandError, ArkivSakTilstandErrorKind, ArkivSakTilstandRepository, ArkivSakTilstand,
};
use async_trait::async_trait;
use sikri_client::Recoverability;

#[derive(Clone)]
pub struct SikriCommandStateRepository;

#[async_trait]
impl ArkivSakTilstandRepository for SikriCommandStateRepository {
    async fn hent_sak_tilstand_fra_arkivet(&self, saksnummer: &str) -> Result<ArkivSakTilstand, ArkivSakTilstandError> {
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
                    let kind = match req_err.status() {
                        Some(status) => match sikri_client::classify_http_error(status, None) {
                            Recoverability::Recoverable => ArkivSakTilstandErrorKind::Recoverable,
                            Recoverability::Irrecoverable => ArkivSakTilstandErrorKind::Irrecoverable,
                        },
                        None => ArkivSakTilstandErrorKind::Recoverable,
                    };
                    let message = match req_err.status() {
                        Some(status) if status == reqwest::StatusCode::NOT_FOUND => {
                            format!("Sak finnes ikke i arkivet ({})", saksnummer)
                        }
                        _ => req_err.to_string(),
                    };
                    return Err(ArkivSakTilstandError::new(kind, message));
                }

                Err(ArkivSakTilstandError::new(
                    ArkivSakTilstandErrorKind::Recoverable,
                    err.to_string(),
                ))
            }
        }
    }
}
