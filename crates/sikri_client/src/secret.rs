use anyhow::{Context, Result, anyhow};
use base64::Engine;
use gcp_auth::Token;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use std::sync::OnceLock;
use std::time::Duration;

const SECRET_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Secret Manager svarer på millisekunder. Arkivets fem minutter ville skjult
/// en død forbindelse hit i fem minutter.
const SECRET_TIMEOUT: Duration = Duration::from_secs(15);

fn secret_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(SECRET_CONNECT_TIMEOUT)
            .timeout(SECRET_TIMEOUT)
            .build()
            .expect("secret-klienten har statisk konfigurasjon")
    })
}

async fn get_token() -> Result<Token> {
    let provider = gcp_auth::provider().await?;
    let scopes = &["https://www.googleapis.com/auth/cloud-platform"];
    let token = provider.token(scopes).await?;

    let token = (*token).clone();

    Ok(token)
}

pub async fn get_secret(
    project_id: &str,
    secret_id: &str,
    version: Option<&str>,
) -> Result<String> {
    let version = version.unwrap_or("latest");
    let url = format!(
        "https://secretmanager.googleapis.com/v1/projects/{project_id}/secrets/{secret_id}/versions/{version}:access"
    );

    let token = get_token().await?;
    let bearer = format!("Bearer {}", token.as_str());

    let resp = secret_client()
        .get(&url)
        .header(AUTHORIZATION, bearer)
        .header(CONTENT_TYPE, "application/json")
        .send()
        .await
        .context("Kunne ikke kalle Secret Manager API")?
        .error_for_status()
        .context("Secret Manager API returnerte en feilstatus")?;

    #[derive(Deserialize)]
    struct AccessResponse {
        payload: Payload,
    }
    #[derive(Deserialize)]
    struct Payload {
        data: String, // base64-kodet
    }

    let AccessResponse {
        payload: Payload { data },
    } = resp.json().await.context("Kunne ikke tolke respons")?;

    // Base64-dekoding og UTF-8-konvertering
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| anyhow!("Base64-dekoding feilet: {e}"))?;
    let secret =
        String::from_utf8(bytes).map_err(|e| anyhow!("Ugyldig UTF-8 i hemmelighet: {e}"))?;

    Ok(secret)
}
