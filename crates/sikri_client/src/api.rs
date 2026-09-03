use crate::AvskrivJournalpost;
use crate::dto::elements_dokument::ElementsDokument;
use crate::dto::elements_dokument_response::ElementsDokumentRespons;
use crate::dto::elements_journalpost::{ElementsJournalpost, ElementsJournalpostRespons};
use crate::dto::elements_sak::ElementsSak;
use crate::dto::elements_sak_response::ElementsSakMedJournalposterResponse;
use crate::error_mapping::SikriFeil;
use crate::secret::get_secret;
use reqwest::Client;
use std::env;
use std::sync::OnceLock;
use std::time::Duration;
use tracing::{debug, error, info};

const SIKRI_ERROR_RESPONSE_LOG_CHUNK_BYTES: usize = 60_000;

/// Hvor lenge vi venter på TCP-oppkobling mot arkivet.
const ARKIV_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Taket for et helt arkivkall. Dekker verste dokumentopplasting — 100 MB rå
/// blir ~134 MB base64 i en JSON-body — med margin.
///
/// Uten et tak stopper én hengende forbindelse **all** eksekvering:
/// executoren er enleder via advisory lock, og reqwest har ingen default.
const ARKIV_TIMEOUT: Duration = Duration::from_secs(300);

/// Delt klient for arkivkall. Gir connection pooling og TLS-gjenbruk i
/// tillegg til timeouten.
fn arkiv_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(ARKIV_CONNECT_TIMEOUT)
            .timeout(ARKIV_TIMEOUT)
            .build()
            .expect("arkivklienten har statisk konfigurasjon")
    })
}

fn base_url() -> String {
    env::var("BASE_URL_SIKRI").unwrap_or_else(|_| {
        panic!("Miljøvariabelen BASE_URL_SIKRI er ikke satt. Sett denne før oppstart")
    })
}

fn safe_endpoint_label(url: &str) -> &str {
    url.split('?')
        .next()
        .unwrap_or(url)
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or("unknown")
}

/// Hvorfor kallet feilet, uten URL.
///
/// `reqwest::Error` sitt `Display` inneholder full URL med query-parametre, og
/// der ligger saksnummer. Etiketten her bærer årsaken — timeout, connect, DNS
/// — som er det man faktisk trenger for å skille en nede-Sikri fra en treg
/// Sikri fra en feilkonfigurert URL.
fn transport_arsak(err: &reqwest::Error) -> &'static str {
    if err.is_timeout() {
        "timeout"
    } else if err.is_connect() {
        "connect"
    } else if err.is_redirect() {
        "redirect"
    } else if err.is_body() {
        "body"
    } else if err.is_decode() {
        "decode"
    } else if err.is_builder() {
        "builder"
    } else {
        "unknown"
    }
}

/// Sikri svarte aldri.
fn transportfeil(method: &str, url: &str, err: reqwest::Error) -> SikriFeil {
    let feil = SikriFeil::utilgjengelig();
    let endpoint = safe_endpoint_label(url);
    error!(
        target: "sikri.http",
        method,
        endpoint,
        sikri_error_code = feil.kode,
        sikri_recoverability = feil.recoverability.as_str(),
        sikri_transport_arsak = transport_arsak(&err),
        "Sikri request failed before a response was received"
    );
    // Rå feiltekst bærer URL med query-parametre og logges derfor kun på
    // debug, som den rå error-bodyen ellers i denne filen.
    debug!(target: "sikri.http", method, endpoint, error = %err, "Sikri transport error detail");
    feil
}

/// Sikri svarte 2xx, men i en form vi ikke kjenner igjen.
fn parsefeil(method: &str, url: &str, err: reqwest::Error) -> SikriFeil {
    let feil = SikriFeil::uparsbart_svar();
    let endpoint = safe_endpoint_label(url);
    error!(
        target: "sikri.http",
        method,
        endpoint,
        sikri_error_code = feil.kode,
        sikri_recoverability = feil.recoverability.as_str(),
        sikri_transport_arsak = transport_arsak(&err),
        "Sikri response could not be parsed"
    );
    debug!(target: "sikri.http", method, endpoint, error = %err, "Sikri parse error detail");
    feil
}

async fn ensure_success(
    response: reqwest::Response,
    method: &str,
    url: &str,
) -> Result<reqwest::Response, SikriFeil> {
    let status = response.status();
    let endpoint = safe_endpoint_label(url);
    if status.is_success() {
        info!(target: "sikri.http", method, endpoint, status = %status, "Sikri response received");
        return Ok(response);
    }

    let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<klarte ikke lese respons-body>".to_string());
    let body = body.trim();
    let feil = SikriFeil::fra_http(status, Some(body));

    error!(
        target: "sikri.http",
        method,
        endpoint,
        status = %status,
        response_length = body.len(),
        sikri_error_code = feil.kode,
        sikri_recoverability = feil.recoverability.as_str(),
        "Sikri response returned error status"
    );
    log_sikri_error_response_chunks(
        method,
        endpoint,
        status,
        body,
        feil.kode,
        feil.recoverability.as_str(),
    );

    Err(feil)
}

fn log_sikri_error_response_chunks(
    method: &str,
    endpoint: &str,
    status: reqwest::StatusCode,
    body: &str,
    safe_detail: &str,
    recoverability: &str,
) {
    if body.is_empty() {
        return;
    }

    let chunks = chunk_text_by_bytes(body, SIKRI_ERROR_RESPONSE_LOG_CHUNK_BYTES);
    let chunk_count = chunks.len();

    for (chunk_index, chunk) in chunks.into_iter().enumerate() {
        // Rå Sikri error-body kan inneholde uforutsigbart mye data og logges
        // derfor KUN på debug!-nivå (av i prod). Den trygge error!-linjen over
        // bærer bare safe code, status og lengde.
        debug!(
            target: "sikri.http",
            method,
            endpoint,
            status = %status,
            response_length = body.len(),
            sikri_error_code = safe_detail,
            sikri_recoverability = recoverability,
            sikri_error_response_chunk_index = chunk_index,
            sikri_error_response_chunk_count = chunk_count,
            sikri_error_response_chunk = chunk,
            "Sikri error response body chunk"
        );
    }
}

fn chunk_text_by_bytes(text: &str, max_chunk_bytes: usize) -> Vec<&str> {
    if text.is_empty() {
        return vec![""];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + max_chunk_bytes).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            end = text[start..]
                .char_indices()
                .nth(1)
                .map(|(index, _)| start + index)
                .unwrap_or(text.len());
        }
        chunks.push(&text[start..end]);
        start = end;
    }

    chunks
}

/// Credentials hentes per kall. Feil her er alltid recoverable: en manglende
/// eller utilgjengelig secret er en driftsfeil, og å terminere klientens
/// kommandoer på grunn av vår egen konfigurasjon ville vært verre enn å vente
/// på at noen retter den.
async fn hent_brukernavn_passord_sikri() -> Result<(String, String), SikriFeil> {
    let project_id = env::var("APP_APPLICATION__PROJECT_ID").map_err(|_| {
        error!(
            target: "sikri.secret",
            sikri_error_code = "sikri_secret_unavailable",
            "APP_APPLICATION__PROJECT_ID er ikke satt"
        );
        SikriFeil::secret_utilgjengelig()
    })?;

    let (username, password) = tokio::try_join!(
        get_secret(&project_id, "sikri-api-username", None),
        get_secret(&project_id, "sikri-api-password", None),
    )
    .map_err(|err| {
        let feil = SikriFeil::secret_utilgjengelig();
        error!(
            target: "sikri.secret",
            sikri_error_code = feil.kode,
            sikri_recoverability = feil.recoverability.as_str(),
            "Klarte ikke hente Sikri-credentials fra Secret Manager"
        );
        debug!(target: "sikri.secret", error = ?err, "Secret Manager error detail");
        feil
    })?;

    Ok((username, password))
}

#[tracing::instrument(skip_all, name = "sikri.alive")]
pub async fn alive() -> Result<(), SikriFeil> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;

    let url = format!("{}/api/Archive/Test", base_url());
    info!(target: "sikri.http", method = "GET", endpoint = safe_endpoint_label(&url), "Sending request to Sikri");
    let resp = arkiv_client()
        .get(&url)
        .basic_auth(username, Some(password))
        .send()
        .await
        .map_err(|err| transportfeil("GET", &url, err))?;
    let _ = ensure_success(resp, "GET", &url).await?;

    Ok(())
}

#[tracing::instrument(skip_all, name = "sikri.get_sak")]
pub async fn get_sak(
    saksnummer: &str,
    kildesystem: &str,
    inkluder_journalposter: bool,
) -> Result<ElementsSakMedJournalposterResponse, SikriFeil> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;

    let url = format!("{}/api/Archive/HentArkivsak", base_url());

    let mut params = vec![("kildesystem", kildesystem), ("saksnr", saksnummer)];
    if inkluder_journalposter {
        params.push(("inkluderJournalposter", "true"));
    }

    info!(
        target: "sikri.http",
        method = "GET",
        endpoint = safe_endpoint_label(&url),
        inkluder_journalposter,
        "Sending request to Sikri"
    );

    let resp = arkiv_client()
        .get(&url)
        .query(&params)
        .basic_auth(username, Some(password))
        .send()
        .await
        .map_err(|err| transportfeil("GET", &url, err))?;
    let resp = ensure_success(resp, "GET", &url).await?;

    //FIXME bør definere en egen DTO som er vår interne modell
    let parsed = resp
        .json::<ElementsSakMedJournalposterResponse>()
        .await
        .map_err(|err| parsefeil("GET", &url, err))?;
    debug!(
        target: "sikri.http",
        method = "GET",
        endpoint = safe_endpoint_label(&url),
        "Sikri get_sak response parsed"
    );
    Ok(parsed)
}

/// `GET /api/Archive/HentJournalpost` — henter journalposten med
/// dokumentobjekter.
///
/// Ren observasjon. Brukes av `AvventJournalfort` for å se når RPA har satt
/// journalstatus til `J` (SKU-0016).
#[tracing::instrument(skip_all, name = "sikri.hent_journalpost")]
pub async fn hent_journalpost(
    journalpost_id: i32,
) -> Result<ElementsJournalpostRespons, SikriFeil> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/HentJournalpost", base_url());

    info!(
        target: "sikri.http",
        method = "GET",
        endpoint = safe_endpoint_label(&url),
        "Sending request to Sikri"
    );

    let resp = arkiv_client()
        .get(&url)
        .query(&[("journalpostId", journalpost_id.to_string())])
        .basic_auth(username, Some(password))
        .send()
        .await
        .map_err(|err| transportfeil("GET", &url, err))?;
    let resp = ensure_success(resp, "GET", &url).await?;

    let parsed = resp
        .json::<ElementsJournalpostRespons>()
        .await
        .map_err(|err| parsefeil("GET", &url, err))?;

    Ok(parsed)
}

#[tracing::instrument(skip_all, name = "sikri.create_sak")]
pub async fn create_sak(
    data: ElementsSak,
) -> Result<ElementsSakMedJournalposterResponse, SikriFeil> {
    // Vår egen forhåndsvalidering. Ekte irrecoverable: samme payload vil bli
    // avvist likt hver gang. Meldingen bærer kun lengder, ikke innhold.
    data.validate().map_err(|feil| {
        SikriFeil::irrecoverable(
            "sikri_request_validation_failed",
            format!("Sikri/Elements avviste forespørselen: {feil}"),
        )
    })?;

    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/OpprettArkivsak", base_url());
    info!(
        target: "sikri.http",
        method = "POST",
        endpoint = safe_endpoint_label(&url),
        "Sending OpprettArkivsak request to Sikri"
    );
    let resp = arkiv_client()
        .post(&url)
        .basic_auth(username, Some(password))
        .json(&data)
        .send()
        .await
        .map_err(|err| transportfeil("POST", &url, err))?;
    let resp = ensure_success(resp, "POST", &url).await?;

    let parsed = resp
        .json::<ElementsSakMedJournalposterResponse>()
        .await
        .map_err(|err| parsefeil("POST", &url, err))?;
    debug!(
        target: "sikri.http",
        method = "POST",
        endpoint = safe_endpoint_label(&url),
        "Sikri create_sak response parsed"
    );
    Ok(parsed)
}

#[tracing::instrument(skip_all, name = "sikri.opprett_journalpost")]
pub async fn opprett_journalpost(
    journalpost: ElementsJournalpost,
    saksnummer: &str,
    kildesystem: Option<&str>,
) -> Result<ElementsJournalpostRespons, SikriFeil> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/OpprettJournalpost", base_url());
    info!(
        target: "sikri.http",
        method = "POST",
        endpoint = safe_endpoint_label(&url),
        "Sending OpprettJournalpost request to Sikri"
    );
    let mut request = arkiv_client()
        .post(&url)
        .basic_auth(username, Some(password));
    if let Some(kildesystem) = kildesystem {
        request = request.query(&[("kildesystem", kildesystem)]);
    }
    let resp = request
        .query(&[("saksnr", saksnummer)])
        .json(&journalpost)
        .send()
        .await
        .map_err(|err| transportfeil("POST", &url, err))?;
    let resp = ensure_success(resp, "POST", &url).await?;

    let parsed = resp
        .json::<ElementsJournalpostRespons>()
        .await
        .map_err(|err| parsefeil("POST", &url, err))?;
    debug!(
        target: "sikri.http",
        method = "POST",
        endpoint = safe_endpoint_label(&url),
        "Sikri opprett_journalpost response parsed"
    );
    Ok(parsed)
}

#[tracing::instrument(skip_all, name = "sikri.legg_til_vedlegg", fields(dokument_count = dokumenter.len()))]
pub async fn legg_til_vedlegg(
    journalpost_id: i32,
    dokumenter: Vec<ElementsDokument>,
) -> Result<Vec<ElementsDokumentRespons>, SikriFeil> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/LeggTilVedleggPaaJournalpost", base_url());
    info!(
        target: "sikri.http",
        method = "POST",
        endpoint = safe_endpoint_label(&url),
        dokument_count = dokumenter.len(),
        "Sending LeggTilVedlegg request to Sikri"
    );
    let resp = arkiv_client()
        .post(&url)
        .basic_auth(username, Some(password))
        .query(&[("journalpostId", journalpost_id.to_string())])
        .json(&dokumenter)
        .send()
        .await
        .map_err(|err| transportfeil("POST", &url, err))?;
    let resp = ensure_success(resp, "POST", &url).await?;

    let parsed = resp
        .json::<Vec<ElementsDokumentRespons>>()
        .await
        .map_err(|err| parsefeil("POST", &url, err))?;
    debug!(
        target: "sikri.http",
        method = "POST",
        endpoint = safe_endpoint_label(&url),
        dokument_count = parsed.len(),
        "Sikri legg_til_vedlegg response parsed"
    );
    Ok(parsed)
}

#[tracing::instrument(skip_all, name = "sikri.sett_journalpost_status", fields(journalpost_status = status))]
pub async fn sett_journalpost_status(journalpost_id: i32, status: &str) -> Result<(), SikriFeil> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/SetJournalpostStatus", base_url());
    info!(
        target: "sikri.http",
        method = "PUT",
        endpoint = safe_endpoint_label(&url),
        journalpost_status = status,
        "Sending request to Sikri"
    );
    let resp = arkiv_client()
        .put(&url)
        .basic_auth(username, Some(password))
        .query(&[
            ("journalpostId", journalpost_id.to_string()),
            ("nyJournalstatus", status.to_string()),
        ])
        .send()
        .await
        .map_err(|err| transportfeil("PUT", &url, err))?;
    let _ = ensure_success(resp, "PUT", &url).await?;
    Ok(())
}

#[tracing::instrument(
    skip_all,
    name = "sikri.avskriv_journalpost",
    fields(avskrivingsmaate = request.avskrivingsmaate)
)]
pub async fn avskriv_journalpost(request: AvskrivJournalpost<'_>) -> Result<(), SikriFeil> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    send_avskriv_journalpost(arkiv_client(), &base_url(), &username, &password, request).await
}

async fn send_avskriv_journalpost(
    client: &Client,
    base_url: &str,
    username: &str,
    password: &str,
    request: AvskrivJournalpost<'_>,
) -> Result<(), SikriFeil> {
    let url = format!("{base_url}/api/Archive/SetAvskrivRestanseJournalpost");
    let mut params = Vec::with_capacity(4);
    if let Some(kildesystem) = request.kildesystem {
        params.push(("kildesystem", kildesystem.to_string()));
    }
    params.extend([
        ("journalpostId", request.journalpost_id.to_string()),
        ("avskrivingsmaate", request.avskrivingsmaate.to_string()),
    ]);
    if let Some(merknad) = request.merknad {
        params.push(("merknad", merknad.to_string()));
    }

    info!(
        target: "sikri.http",
        method = "PUT",
        endpoint = safe_endpoint_label(&url),
        "Sending request to Sikri"
    );
    let resp = client
        .put(&url)
        .basic_auth(username, Some(password))
        .query(&params)
        .send()
        .await
        .map_err(|err| transportfeil("PUT", &url, err))?;
    let _ = ensure_success(resp, "PUT", &url).await?;
    Ok(())
}

#[tracing::instrument(skip_all, name = "sikri.avslutt_sak")]
pub async fn avslutt_sak(saksnummer: &str) -> Result<(), SikriFeil> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/SetStatusForArkivSak", base_url());
    info!(
        target: "sikri.http",
        method = "PUT",
        endpoint = safe_endpoint_label(&url),
        "Sending request to Sikri"
    );
    let resp = arkiv_client()
        .put(&url)
        .basic_auth(username, Some(password))
        .query(&[("saksnr", saksnummer), ("nySaksstatus", "A")])
        .send()
        .await
        .map_err(|err| transportfeil("PUT", &url, err))?;
    let _ = ensure_success(resp, "PUT", &url).await?;
    Ok(())
}

#[tracing::instrument(skip_all, name = "sikri.sett_saksansvarlig")]
pub async fn sett_saksansvarlig(
    saksnummer: &str,
    saksbehandler: &str,
    saksbehandler_enhet: &str,
) -> Result<(), SikriFeil> {
    let (username, password) = hent_brukernavn_passord_sikri().await?;
    let url = format!("{}/api/Archive/SetSaksansvarligIdForArkivSak", base_url());
    info!(
        target: "sikri.http",
        method = "PUT",
        endpoint = safe_endpoint_label(&url),
        "Sending SetSaksansvarligIdForArkivSak request to Sikri"
    );
    let resp = arkiv_client()
        .put(&url)
        .basic_auth(username, Some(password))
        .query(&[
            ("saksnr", saksnummer),
            ("saksbehandler", saksbehandler),
            ("saksbehandlerEnhet", saksbehandler_enhet),
        ])
        .send()
        .await
        .map_err(|err| transportfeil("PUT", &url, err))?;
    let _ = ensure_success(resp, "PUT", &url).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn start_mock_sikri(status: &str) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_string();
        let request = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let bytes_read = stream.read(&mut chunk).await.unwrap();
                if bytes_read == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..bytes_read]);
                if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            stream
                .write_all(
                    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                        .as_bytes(),
                )
                .await
                .unwrap();
            String::from_utf8(buffer).unwrap()
        });

        (format!("http://{address}"), request)
    }

    #[tokio::test]
    async fn avskriv_journalpost_bruker_canonical_http_contract_med_encoding() {
        let (base_url, received_request) = start_mock_sikri("200 OK").await;

        send_avskriv_journalpost(
            &Client::new(),
            &base_url,
            "bruker",
            "passord",
            AvskrivJournalpost {
                journalpost_id: 123,
                avskrivingsmaate: "T/E",
                kildesystem: Some("Skuffen & fagsystem"),
                merknad: Some("Tatt til etterretning: æ"),
            },
        )
        .await
        .unwrap();

        let request = received_request.await.unwrap();
        let request_line = request.lines().next().unwrap();
        assert_eq!(
            request_line,
            "PUT /api/Archive/SetAvskrivRestanseJournalpost?kildesystem=Skuffen+%26+fagsystem&journalpostId=123&avskrivingsmaate=T%2FE&merknad=Tatt+til+etterretning%3A+%C3%A6 HTTP/1.1"
        );
    }

    #[tokio::test]
    async fn avskriv_journalpost_klassifiserer_404_som_not_found() {
        let (base_url, received_request) = start_mock_sikri("404 Not Found").await;

        let feil = send_avskriv_journalpost(
            &Client::new(),
            &base_url,
            "bruker",
            "passord",
            AvskrivJournalpost {
                journalpost_id: 404,
                avskrivingsmaate: "TE",
                kildesystem: None,
                merknad: None,
            },
        )
        .await
        .unwrap_err();

        let request = received_request.await.unwrap();
        assert_eq!(
            request.lines().next().unwrap(),
            "PUT /api/Archive/SetAvskrivRestanseJournalpost?journalpostId=404&avskrivingsmaate=TE HTTP/1.1"
        );
        assert_eq!(feil.kode, "sikri_resource_not_found");
        assert_eq!(feil.recoverability, crate::Recoverability::Irrecoverable);
    }

    #[test]
    fn chunks_error_response_without_splitting_utf8() {
        let text = "å".repeat(5);
        let chunks = chunk_text_by_bytes(&text, 3);

        assert_eq!(chunks, vec!["å", "å", "å", "å", "å"]);
    }

    #[test]
    fn chunks_empty_error_response_as_one_empty_chunk() {
        let chunks = chunk_text_by_bytes("", 60_000);

        assert_eq!(chunks, vec![""]);
    }
}
