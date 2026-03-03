use application::command::ports::command_state_port::{
    CommandStateError, CommandStateErrorKind, CommandStateRepository, SakState,
};
use async_trait::async_trait;
use sikri_client::Recoverability;

#[derive(Clone)]
pub struct SikriCommandStateRepository;

#[async_trait]
impl CommandStateRepository for SikriCommandStateRepository {
    async fn hent_sak_state(&self, saksnummer: &str) -> Result<SakState, CommandStateError> {
        match sikri_client::hent_sak(saksnummer, "SKUFFEN", false).await {
            Ok(sak) => {
                let avsluttet = sak.lukket
                    || sak
                        .saksstatus
                        .as_deref()
                        .and_then(|status| status.chars().next())
                        == Some('A');
                Ok(SakState { avsluttet })
            }
            Err(err) => {
                if let Some(req_err) = err.downcast_ref::<reqwest::Error>() {
                    let kind = match req_err.status() {
                        Some(status) => match sikri_client::classify_http_error(status, None) {
                            Recoverability::Recoverable => CommandStateErrorKind::Recoverable,
                            Recoverability::Irrecoverable => CommandStateErrorKind::Irrecoverable,
                        },
                        None => CommandStateErrorKind::Recoverable,
                    };
                    let message = match req_err.status() {
                        Some(status) if status == reqwest::StatusCode::NOT_FOUND => {
                            format!("Sak finnes ikke i Sikri ({})", saksnummer)
                        }
                        _ => req_err.to_string(),
                    };
                    return Err(CommandStateError::new(kind, message));
                }

                Err(CommandStateError::new(
                    CommandStateErrorKind::Recoverable,
                    err.to_string(),
                ))
            }
        }
    }
}
