use crate::dto::elements_sak::ElementsSak;
use crate::dto::elements_sak_response::ElementsSakMedJournalposterResponse;
use crate::secret::get_secret;
use anyhow::{Context, Result};
use reqwest::Client;
use std::env;

fn base_url() -> String {
    env::var("BASE_URL_SIKRI").unwrap_or_else(|_| {
        panic!("Miljøvariabelen BASE_URL_DIKRI er ikke satt. Sett denne før oppstart")
    })
}

async fn hent_brukernavn_passord_sikri() -> Result<(String, String)> {
    let project_id = env::var("APP_APPLICATION__PROJECT_ID")?;

    let (username, password) = tokio::try_join!(
        get_secret(&project_id, "sikri-api-cloud-username", None),
        get_secret(&project_id, "sikri-api-cloud-password", None),
    )?;

    Ok((username, password))
}

pub async fn alive() -> Result<()> {
    let (username, password) = hent_brukernavn_passord_sikri()
        .await
        .context("Feil ved henting av Sikri-brukernavn/passord (GCP secret)")?;

    let url = format!("{}/api/Archive/Test", base_url());
    Client::new()
        .get(&url)
        .basic_auth(username, Some(password))
        .send()
        .await
        .with_context(|| format!("Klarte ikke å sende request til {}", url))?
        .error_for_status()
        .with_context(|| format!("Server svarte med feil for GET {}", url))?;

    Ok(())
}

pub async fn get_sak(
    saksnummer: &str,
    kildesystem: &str,
    inkluder_journalposter: bool,
) -> Result<ElementsSakMedJournalposterResponse> {
    let (username, password) = hent_brukernavn_passord_sikri()
        .await
        .context("Feil ved henting av Sikri-brukernavn/passord (GCP secret)")?;

    let url = format!("{}/api/Archive/HentArkivsak", base_url());

    let mut params = vec![("kildesystem", kildesystem), ("saksnr", saksnummer)];
    if inkluder_journalposter {
        params.push(("inkluderJournalposter", "true"));
    }

    let resp = Client::new()
        .get(&url)
        .query(&params)
        .basic_auth(username, Some(password))
        .send()
        .await
        .with_context(|| {
            format!(
                "Klarte ikke å sende request til {} (kildesystem={}, saksnr={})",
                url, kildesystem, saksnummer
            )
        })?
        // Viktig: bevar reqwest::Error slik at .status() kan leses i tester
        .error_for_status()
        .with_context(|| {
            format!(
                "Server svarte med feil for GET {} (kildesystem={}, saksnr={})",
                url, kildesystem, saksnummer
            )
        })?;

    //FIXME bør definere en egen DTO som er vår interne modell
    resp.json::<ElementsSakMedJournalposterResponse>()
        .await
        .with_context(|| "Feil ved parsing av JSON-respons for get_sak()")
}

pub async fn create_sak(data: ElementsSak) -> Result<ElementsSakMedJournalposterResponse> {
    let _ = data.validate();

    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/OpprettArkivsak", base_url());
    let resp = Client::new()
        .post(&url)
        .basic_auth(username, Some(password))
        .json(&data)
        .send()
        .await
        .with_context(|| format!("Klarte ikke å sende request til {}", url))?
        .error_for_status()
        .with_context(|| format!("Server svarte med feil for POST {}", url))?;

    resp.json::<ElementsSakMedJournalposterResponse>()
        .await
        .with_context(|| "Feil ved parsing av JSON-respons for create_sak()")
}
