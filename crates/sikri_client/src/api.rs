use crate::dto::elements_dokument::ElementsDokument;
use crate::dto::elements_dokument_response::ElementsDokumentRespons;
use crate::dto::elements_journalpost::{ElementsJournalpost, ElementsJournalpostRespons};
use crate::dto::elements_sak::ElementsSak;
use crate::dto::elements_sak_response::ElementsSakMedJournalposterResponse;
use crate::error_mapping::{classify_http_error, marker_for, user_message_for_http_error};
use crate::secret::get_secret;
use anyhow::{Context, Result};
use reqwest::Client;
use serde::Serialize;
use std::env;
use tracing::{error, info};

fn base_url() -> String {
    env::var("BASE_URL_SIKRI").unwrap_or_else(|_| {
        panic!("Miljøvariabelen BASE_URL_SIKRI er ikke satt. Sett denne før oppstart")
    })
}

async fn ensure_success(
    response: reqwest::Response,
    method: &str,
    url: &str,
) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        info!(target: "sikri.http", method, url, status = %status, "Sikri response received");
        return Ok(response);
    }

    let body = response
        .text()
        .await
        .unwrap_or_else(|err| format!("<klarte ikke lese respons-body: {err}>"));
    let body = body.trim();
    let body = if body.len() > 2000 {
        format!("{}...<truncated>", &body[..2000])
    } else {
        body.to_string()
    };
    let recoverability = classify_http_error(status, Some(&body));
    let marker = marker_for(recoverability);
    let user_message = user_message_for_http_error(status, Some(&body));

    error!(
        target: "sikri.http",
        method,
        url,
        status = %status,
        response_body = %body,
        "Sikri response returned error status"
    );

    anyhow::bail!("{marker} {user_message} (method={method}, url={url}, status={status})");
}

fn truncate_for_log(raw: &str, max_len: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.len() <= max_len {
        return trimmed.to_string();
    }

    format!("{}...<truncated>", &trimmed[..max_len])
}

fn payload_for_log<T: Serialize>(payload: &T) -> String {
    match serde_json::to_string(payload) {
        Ok(json) => truncate_for_log(&json, 2000),
        Err(err) => format!("<failed to serialize payload: {err}>"),
    }
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
    info!(target: "sikri.http", method = "GET", url = %url, "Sending request to Sikri");
    let resp = Client::new()
        .get(&url)
        .basic_auth(username, Some(password))
        .send()
        .await
        .with_context(|| format!("Klarte ikke å sende request til {url}"))?;
    let _ = ensure_success(resp, "GET", &url)
        .await
        .with_context(|| format!("Server svarte med feil for GET {url}"))?;

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

    info!(
        target: "sikri.http",
        method = "GET",
        url = %url,
        kildesystem,
        saksnr = saksnummer,
        inkluder_journalposter,
        "Sending request to Sikri"
    );

    let resp = Client::new()
        .get(&url)
        .query(&params)
        .basic_auth(username, Some(password))
        .send()
        .await
        .with_context(|| {
            format!(
                "Klarte ikke å sende request til {url} (kildesystem={kildesystem}, saksnr={saksnummer})"
            )
        })?;
    let resp = ensure_success(resp, "GET", &url).await.with_context(|| {
        format!(
            "Server svarte med feil for GET {url} (kildesystem={kildesystem}, saksnr={saksnummer})"
        )
    })?;

    //FIXME bør definere en egen DTO som er vår interne modell
    let parsed = resp
        .json::<ElementsSakMedJournalposterResponse>()
        .await
        .with_context(|| "Feil ved parsing av JSON-respons for get_sak()")?;
    info!(
        target: "sikri.http",
        method = "GET",
        url = %url,
        response_body = %payload_for_log(&parsed),
        "Sikri response payload"
    );
    Ok(parsed)
}

pub async fn create_sak(data: ElementsSak) -> Result<ElementsSakMedJournalposterResponse> {
    let _ = data.validate();

    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/OpprettArkivsak", base_url());
    info!(
        target: "sikri.http",
        method = "POST",
        url = %url,
        request_body = %payload_for_log(&data),
        "Sending request to Sikri"
    );
    let resp = Client::new()
        .post(&url)
        .basic_auth(username, Some(password))
        .json(&data)
        .send()
        .await
        .with_context(|| format!("Klarte ikke å sende request til {url}"))?;
    let resp = ensure_success(resp, "POST", &url).await?;

    let parsed = resp
        .json::<ElementsSakMedJournalposterResponse>()
        .await
        .with_context(|| "Feil ved parsing av JSON-respons for create_sak()")?;
    info!(
        target: "sikri.http",
        method = "POST",
        url = %url,
        response_body = %payload_for_log(&parsed),
        "Sikri response payload"
    );
    Ok(parsed)
}

pub async fn opprett_journalpost(
    journalpost: ElementsJournalpost,
    saksnummer: &str,
) -> Result<ElementsJournalpostRespons> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/OpprettJournalpost", base_url());
    info!(
        target: "sikri.http",
        method = "POST",
        url = %url,
        saksnr = saksnummer,
        request_body = %payload_for_log(&journalpost),
        "Sending request to Sikri"
    );
    let resp = Client::new()
        .post(&url)
        .basic_auth(username, Some(password))
        .query(&[("saksnr", saksnummer)])
        .json(&journalpost)
        .send()
        .await
        .with_context(|| format!("Klarte ikke å sende request til {url}"))?;
    let resp = ensure_success(resp, "POST", &url).await?;

    let parsed = resp
        .json::<ElementsJournalpostRespons>()
        .await
        .with_context(|| "Feil ved parsing av JSON-respons for opprett_journalpost()")?;
    info!(
        target: "sikri.http",
        method = "POST",
        url = %url,
        response_body = %payload_for_log(&parsed),
        "Sikri response payload"
    );
    Ok(parsed)
}

pub async fn legg_til_vedlegg(
    journalpost_id: i32,
    dokumenter: Vec<ElementsDokument>,
) -> Result<Vec<ElementsDokumentRespons>> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/LeggTilVedlegg", base_url());
    info!(
        target: "sikri.http",
        method = "POST",
        url = %url,
        journalpost_id,
        request_body = %payload_for_log(&dokumenter),
        "Sending request to Sikri"
    );
    let resp = Client::new()
        .post(&url)
        .basic_auth(username, Some(password))
        .query(&[("journalpostId", journalpost_id.to_string())])
        .json(&dokumenter)
        .send()
        .await
        .with_context(|| format!("Klarte ikke å sende request til {url}"))?;
    let resp = ensure_success(resp, "POST", &url).await?;

    let parsed = resp
        .json::<Vec<ElementsDokumentRespons>>()
        .await
        .with_context(|| "Feil ved parsing av JSON-respons for legg_til_vedlegg()")?;
    info!(
        target: "sikri.http",
        method = "POST",
        url = %url,
        response_body = %payload_for_log(&parsed),
        "Sikri response payload"
    );
    Ok(parsed)
}

pub async fn sett_journalpost_status(journalpost_id: i32, status: &str) -> Result<()> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/SettJournalpostStatus", base_url());
    info!(
        target: "sikri.http",
        method = "POST",
        url = %url,
        journalpost_id,
        journalpost_status = status,
        "Sending request to Sikri"
    );
    let resp = Client::new()
        .post(&url)
        .basic_auth(username, Some(password))
        .query(&[
            ("journalpostId", journalpost_id.to_string()),
            ("status", status.to_string()),
        ])
        .send()
        .await
        .with_context(|| format!("Klarte ikke å sende request til {url}"))?;
    let _ = ensure_success(resp, "POST", &url).await?;
    Ok(())
}

pub async fn avskriv_journalpost(journalpost_id: i32, avskrivingsmaate: &str) -> Result<()> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/AvskrivJournalpost", base_url());
    info!(
        target: "sikri.http",
        method = "POST",
        url = %url,
        journalpost_id,
        avskrivingsmaate,
        "Sending request to Sikri"
    );
    let resp = Client::new()
        .post(&url)
        .basic_auth(username, Some(password))
        .query(&[
            ("journalpostId", journalpost_id.to_string()),
            ("avskrivingsmaate", avskrivingsmaate.to_string()),
        ])
        .send()
        .await
        .with_context(|| format!("Klarte ikke å sende request til {url}"))?;
    let _ = ensure_success(resp, "POST", &url).await?;
    Ok(())
}

pub async fn avslutt_sak(saksnummer: &str) -> Result<()> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/AvsluttArkivsak", base_url());
    info!(
        target: "sikri.http",
        method = "POST",
        url = %url,
        saksnr = saksnummer,
        "Sending request to Sikri"
    );
    let resp = Client::new()
        .post(&url)
        .basic_auth(username, Some(password))
        .query(&[("saksnr", saksnummer)])
        .send()
        .await
        .with_context(|| format!("Klarte ikke å sende request til {url}"))?;
    let _ = ensure_success(resp, "POST", &url).await?;
    Ok(())
}
