use std::time::Duration;

use application::command::ports::dokument_renderer_port::{
    DokumentRenderer, RendererFeil, RendererKontekst,
};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE};
use tracing::{debug, error, info, warn};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const ERROR_MESSAGE_CHARS: usize = 500;
const EXTERNAL_RESPONSE_MESSAGE_CHARS: usize = 500;
const EXTERNAL_RESPONSE_MESSAGE_BYTES: usize = 2048;

#[async_trait]
pub trait IdTokenProvider: Send + Sync {
    async fn id_token(&self, audience: &str) -> Result<String, RendererFeil>;
}

pub struct GcpIdTokenProvider;

#[async_trait]
impl IdTokenProvider for GcpIdTokenProvider {
    async fn id_token(&self, audience: &str) -> Result<String, RendererFeil> {
        let audience_label = safe_audience_label(audience);
        let url = metadata_identity_url(audience);
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(CONNECT_TIMEOUT)
            .build()
            .map_err(|err| {
                let error_message = sanitize_error_message(&err.to_string());
                warn!(
                    event = "html2pdf_auth_failed",
                    category = "gcp_token_provider",
                    error_class = "metadata_client",
                    audience = %audience_label,
                    error_message = %error_message,
                    "html2pdf token client initialization failed"
                );
                RendererFeil::irrecoverable(format!(
                    "html2pdf_auth_failed category=gcp_token_provider audience={} error_class=metadata_client error_message=\"{}\"",
                    audience_label,
                    quote_value(&error_message)
                ))
            })?;

        let response = client
            .get(url)
            .header("Metadata-Flavor", "Google")
            .send()
            .await
            .map_err(|err| {
                let error_message = sanitize_error_message(&err.to_string());
                warn!(
                    event = "html2pdf_auth_failed",
                    category = "gcp_token_request",
                    error_class = "metadata_request",
                    audience = %audience_label,
                    error_message = %error_message,
                    "html2pdf metadata identity token request failed"
                );
                RendererFeil::recoverable(format!(
                    "html2pdf_auth_failed category=gcp_token_request audience={} error_class=metadata_request error_message=\"{}\"",
                    audience_label,
                    quote_value(&error_message)
                ))
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            warn!(
                event = "html2pdf_auth_failed",
                category = "gcp_token_request",
                error_class = "metadata_status",
                audience = %audience_label,
                status_code = status,
                "html2pdf metadata identity token request returned non-success"
            );
            return Err(RendererFeil::recoverable(format!(
                "html2pdf_auth_failed category=gcp_token_request audience={} error_class=metadata_status status={}",
                audience_label, status
            )));
        }

        let token = response.text().await.map_err(|err| {
            let error_message = sanitize_error_message(&err.to_string());
            warn!(
                event = "html2pdf_auth_failed",
                category = "gcp_token_request",
                error_class = "metadata_body",
                audience = %audience_label,
                error_message = %error_message,
                "html2pdf metadata identity token response read failed"
            );
            RendererFeil::recoverable(format!(
                "html2pdf_auth_failed category=gcp_token_request audience={} error_class=metadata_body error_message=\"{}\"",
                audience_label,
                quote_value(&error_message)
            ))
        })?;

        let token = token.trim().to_string();
        if token.is_empty() {
            warn!(
                event = "html2pdf_auth_failed",
                category = "gcp_token_request",
                error_class = "metadata_empty",
                audience = %audience_label,
                "html2pdf metadata identity token response was empty"
            );
            return Err(RendererFeil::recoverable(format!(
                "html2pdf_auth_failed category=gcp_token_request audience={} error_class=metadata_empty",
                audience_label
            )));
        }

        Ok(token)
    }
}

fn metadata_identity_url(audience: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(audience.as_bytes()).collect();
    format!(
        "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/identity?audience={}",
        encoded
    )
}

pub struct Html2PdfRendererAdapter {
    client: reqwest::Client,
    endpoint: String,
    audience: String,
    token_provider: Box<dyn IdTokenProvider>,
}

impl Html2PdfRendererAdapter {
    pub fn new(
        endpoint: String,
        audience: String,
        token_provider: Box<dyn IdTokenProvider>,
    ) -> Self {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("valid reqwest client config");

        Self {
            client,
            endpoint,
            audience,
            token_provider,
        }
    }

    fn diagnostics_context(&self) -> RendererDiagnosticsContext {
        RendererDiagnosticsContext {
            endpoint_host: safe_url_host(&self.endpoint),
            endpoint_path: safe_url_path(&self.endpoint),
            endpoint_label: safe_url_label(&self.endpoint),
            audience: safe_audience_label(&self.audience),
        }
    }
}

#[derive(Debug, Clone)]
struct RendererDiagnosticsContext {
    endpoint_host: String,
    endpoint_path: String,
    endpoint_label: String,
    audience: String,
}

#[async_trait]
impl DokumentRenderer for Html2PdfRendererAdapter {
    async fn render(
        &self,
        html: &[u8],
        kontekst: RendererKontekst,
    ) -> Result<Vec<u8>, RendererFeil> {
        let context = self.diagnostics_context();
        let html_byte_len = html.len();
        let correlation_id = format_optional_uuid(kontekst.correlation_id);

        info!(
            event = "html2pdf_request_start",
            command_id = %kontekst.command_id,
            correlation_id = %correlation_id,
            journalpost_id = %kontekst.journalpost_id.0,
            dokument_id = %kontekst.dokument_id.0,
            endpoint_host = %context.endpoint_host,
            endpoint_path = %context.endpoint_path,
            endpoint_label = %context.endpoint_label,
            audience = %context.audience,
            html_byte_len,
            timeout_category = "connect_5s_total_60s",
            "html2pdf render request started"
        );

        let token = match self.token_provider.id_token(&self.audience).await {
            Ok(token) => token,
            Err(err) => {
                let error_message = sanitize_error_message(err.safe_message());
                error!(
                    event = "html2pdf_auth_failed",
                    command_id = %kontekst.command_id,
                    correlation_id = %correlation_id,
                    journalpost_id = %kontekst.journalpost_id.0,
                    dokument_id = %kontekst.dokument_id.0,
                    category = "token_acquisition",
                    endpoint_host = %context.endpoint_host,
                    endpoint_path = %context.endpoint_path,
                    audience = %context.audience,
                    error_message = %error_message,
                    "html2pdf token acquisition failed before request"
                );
                return Err(err);
            }
        };

        let response = match self
            .client
            .post(&self.endpoint)
            .header(CONTENT_TYPE, "text/html; charset=utf-8")
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .body(html.to_vec())
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                let category = classify_network_error(&err);
                let error_message = sanitize_error_message(&err.to_string());
                warn!(
                    event = "html2pdf_request_failed",
                    command_id = %kontekst.command_id,
                    correlation_id = %correlation_id,
                    journalpost_id = %kontekst.journalpost_id.0,
                    dokument_id = %kontekst.dokument_id.0,
                    category,
                    endpoint_host = %context.endpoint_host,
                    endpoint_path = %context.endpoint_path,
                    endpoint_label = %context.endpoint_label,
                    audience = %context.audience,
                    error_message = %error_message,
                    "html2pdf renderer request failed"
                );
                return Err(RendererFeil::recoverable(format!(
                    "html2pdf_request_failed category={} endpoint_host={} endpoint_path={} audience={} error_message=\"{}\"",
                    category,
                    context.endpoint_host,
                    context.endpoint_path,
                    context.audience,
                    quote_value(&error_message)
                )));
            }
        };

        let status = response.status();
        let status_code = status.as_u16();
        let status_class = classify_status(status_code);
        let content_type = safe_header_value(response.headers().get(CONTENT_TYPE));
        let content_length = safe_header_value(response.headers().get(CONTENT_LENGTH));

        debug!(
            event = "html2pdf_response_received",
            command_id = %kontekst.command_id,
            correlation_id = %correlation_id,
            journalpost_id = %kontekst.journalpost_id.0,
            dokument_id = %kontekst.dokument_id.0,
            status_code,
            status_class,
            content_type = %content_type,
            content_length = %content_length,
            endpoint_host = %context.endpoint_host,
            endpoint_path = %context.endpoint_path,
            "html2pdf renderer response received"
        );

        if status.is_success() {
            return response.bytes().await.map(|bytes| bytes.to_vec()).map_err(|err| {
                let category = classify_network_error(&err);
                let error_message = sanitize_error_message(&err.to_string());
                error!(
                    event = "html2pdf_response_read_failed",
                    command_id = %kontekst.command_id,
                    correlation_id = %correlation_id,
                    journalpost_id = %kontekst.journalpost_id.0,
                    dokument_id = %kontekst.dokument_id.0,
                    category,
                    status_code,
                    status_class,
                    endpoint_host = %context.endpoint_host,
                    endpoint_path = %context.endpoint_path,
                    error_message = %error_message,
                    "html2pdf success response body could not be read"
                );
                RendererFeil::recoverable(format!(
                    "html2pdf_response_read_failed category={} status={} status_class={} endpoint_host={} endpoint_path={} audience={} error_message=\"{}\"",
                    category,
                    status_code,
                    status_class,
                    context.endpoint_host,
                    context.endpoint_path,
                    context.audience,
                    quote_value(&error_message)
                ))
            });
        }

        let category = classify_error_category(status_code);
        let prefix = renderer_error_prefix(status_code);
        let external_error_message =
            read_external_response_error_message(status_code, &content_type, response).await;
        let external_error_message_value =
            external_error_message.as_deref().unwrap_or("suppressed");
        let diagnostic = format!(
            "{} category={} status={} status_class={} endpoint_host={} endpoint_path={} audience={} content_type=\"{}\" content_length={} external_error_message=\"{}\"",
            prefix,
            category,
            status_code,
            status_class,
            context.endpoint_host,
            context.endpoint_path,
            context.audience,
            quote_value(&content_type),
            content_length,
            quote_value(external_error_message_value)
        );

        match category {
            "auth_failure" => {
                warn!(
                    event = "html2pdf_auth_failed",
                    command_id = %kontekst.command_id,
                    correlation_id = %correlation_id,
                    journalpost_id = %kontekst.journalpost_id.0,
                    dokument_id = %kontekst.dokument_id.0,
                    category,
                    status_code,
                    status_class,
                    content_type = %content_type,
                    content_length = %content_length,
                    endpoint_host = %context.endpoint_host,
                    endpoint_path = %context.endpoint_path,
                    audience = %context.audience,
                    external_error_message = %external_error_message_value,
                    "html2pdf renderer auth failure"
                );
            }
            "client_error" => {
                error!(
                    event = "html2pdf_client_error",
                    command_id = %kontekst.command_id,
                    correlation_id = %correlation_id,
                    journalpost_id = %kontekst.journalpost_id.0,
                    dokument_id = %kontekst.dokument_id.0,
                    category,
                    status_code,
                    status_class,
                    content_type = %content_type,
                    content_length = %content_length,
                    endpoint_host = %context.endpoint_host,
                    endpoint_path = %context.endpoint_path,
                    audience = %context.audience,
                    external_error_message = %external_error_message_value,
                    "html2pdf renderer returned client error"
                );
            }
            "server_error" => {
                error!(
                    event = "html2pdf_server_error",
                    command_id = %kontekst.command_id,
                    correlation_id = %correlation_id,
                    journalpost_id = %kontekst.journalpost_id.0,
                    dokument_id = %kontekst.dokument_id.0,
                    category,
                    status_code,
                    status_class,
                    content_type = %content_type,
                    content_length = %content_length,
                    endpoint_host = %context.endpoint_host,
                    endpoint_path = %context.endpoint_path,
                    audience = %context.audience,
                    external_error_message = %external_error_message_value,
                    "html2pdf renderer returned server error"
                );
            }
            _ => {
                warn!(
                    event = "html2pdf_request_failed",
                    command_id = %kontekst.command_id,
                    correlation_id = %correlation_id,
                    journalpost_id = %kontekst.journalpost_id.0,
                    dokument_id = %kontekst.dokument_id.0,
                    category,
                    status_code,
                    status_class,
                    content_type = %content_type,
                    content_length = %content_length,
                    endpoint_host = %context.endpoint_host,
                    endpoint_path = %context.endpoint_path,
                    audience = %context.audience,
                    external_error_message = %external_error_message_value,
                    "html2pdf renderer returned unexpected status"
                );
            }
        }

        let err_kind = match category {
            "auth_failure" | "server_error" | "unexpected_status" => {
                RendererFeil::recoverable(diagnostic)
            }
            "client_error" => RendererFeil::irrecoverable(diagnostic),
            _ => RendererFeil::recoverable(diagnostic),
        };
        Err(err_kind)
    }
}

fn classify_error_category(status_code: u16) -> &'static str {
    match status_code {
        401 | 403 => "auth_failure",
        400..=499 => "client_error",
        500..=599 => "server_error",
        _ => "unexpected_status",
    }
}

fn classify_status(status_code: u16) -> &'static str {
    match status_code {
        100..=199 => "informational",
        200..=299 => "success",
        300..=399 => "redirect",
        400..=499 => "client_error",
        500..=599 => "server_error",
        _ => "other",
    }
}

fn renderer_error_prefix(status_code: u16) -> &'static str {
    match status_code {
        401 | 403 => "html2pdf_auth_failed",
        400..=499 => "html2pdf_client_error",
        500..=599 => "html2pdf_server_error",
        _ => "html2pdf_request_failed",
    }
}

fn safe_url_host(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(ToString::to_string))
        .unwrap_or_else(|| "unknown".to_string())
}

fn safe_url_path(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .map(|parsed| {
            let path = parsed.path();
            if path.is_empty() {
                "/".to_string()
            } else {
                path.to_string()
            }
        })
        .unwrap_or_else(|| "/".to_string())
}

fn safe_url_label(url: &str) -> String {
    let host = safe_url_host(url);
    let path = safe_url_path(url);
    format!("{host}{path}")
}

fn safe_audience_label(audience: &str) -> String {
    safe_url_host(audience)
}

fn safe_header_value(value: Option<&reqwest::header::HeaderValue>) -> String {
    value
        .and_then(|value| value.to_str().ok())
        .map(sanitize_error_message)
        .unwrap_or_else(|| "unknown".to_string())
}

async fn read_external_response_error_message(
    status_code: u16,
    content_type: &str,
    mut response: reqwest::Response,
) -> Option<String> {
    if matches!(status_code, 401 | 403) || !content_type_allows_external_message(content_type) {
        return None;
    }

    let mut bytes = Vec::new();
    while bytes.len() < EXTERNAL_RESPONSE_MESSAGE_BYTES {
        let Some(chunk) = response.chunk().await.ok().flatten() else {
            break;
        };
        let remaining = EXTERNAL_RESPONSE_MESSAGE_BYTES - bytes.len();
        bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }

    let text = String::from_utf8_lossy(&bytes);
    let message = sanitize_external_response_message(&text);
    (!message.is_empty()).then_some(message)
}

fn content_type_allows_external_message(content_type: &str) -> bool {
    let content_type = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();

    matches!(content_type.as_str(), "text/plain" | "application/json")
}

fn sanitize_external_response_message(message: &str) -> String {
    truncate_chars(
        sanitize_error_message(message).trim(),
        EXTERNAL_RESPONSE_MESSAGE_CHARS,
    )
}

fn sanitize_error_message(message: &str) -> String {
    let normalized = normalize_control_chars(message);
    let without_queries = strip_url_queries(&normalized);
    let redacted = redact_sensitive_tokens(&without_queries);
    truncate_chars(redacted.trim(), ERROR_MESSAGE_CHARS)
}

fn format_optional_uuid(id: Option<uuid::Uuid>) -> String {
    id.map(|id| id.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn normalize_control_chars(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn strip_url_queries(value: &str) -> String {
    value
        .split_whitespace()
        .map(|token| {
            token
                .find('?')
                .map(|index| format!("{}?redacted", &token[..index]))
                .unwrap_or_else(|| token.to_string())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_sensitive_tokens(value: &str) -> String {
    let mut redacted = Vec::new();
    let mut redact_next = 0;

    for token in value.split_whitespace() {
        if redact_next > 0 {
            redacted.push("redacted".to_string());
            redact_next -= 1;
            continue;
        }

        let lower = token.to_ascii_lowercase();
        if is_sensitive_token(&lower) {
            redacted.push(redact_sensitive_token(token));
            redact_next = sensitive_following_token_count(token, &lower);
        } else {
            redacted.push(token.to_string());
        }
    }

    redacted.join(" ")
}

fn is_sensitive_token(lower: &str) -> bool {
    lower.contains("authorization")
        || lower == "bearer"
        || lower.starts_with("bearer=")
        || lower == "basic"
        || lower.starts_with("basic=")
        || lower.contains("password")
        || lower.contains("passwd")
        || lower.contains("credential")
        || lower.contains("token")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("x-api-key")
        || lower.contains("secret")
}

fn sensitive_following_token_count(token: &str, lower: &str) -> usize {
    if lower == "bearer" || lower == "basic" {
        1
    } else if lower.contains("authorization") && token.ends_with(':') {
        2
    } else if token.ends_with(':') || token == "=" {
        1
    } else {
        0
    }
}

fn redact_sensitive_token(token: &str) -> String {
    token
        .find(['=', ':'])
        .map(|index| format!("{}redacted", &token[..=index]))
        .unwrap_or_else(|| "redacted".to_string())
}

fn quote_value(value: &str) -> String {
    value.replace('"', "'")
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut truncated: String = value.chars().take(max_chars).collect();
    if value.chars().count() > max_chars {
        truncated.push_str("...");
    }
    truncated
}

fn classify_network_error(error: &reqwest::Error) -> &'static str {
    if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_request() {
        "request"
    } else if error.is_body() {
        "body"
    } else if error.is_decode() {
        "decode"
    } else {
        "unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_status_groups_status_codes() {
        assert_eq!(classify_status(100), "informational");
        assert_eq!(classify_status(200), "success");
        assert_eq!(classify_status(302), "redirect");
        assert_eq!(classify_status(404), "client_error");
        assert_eq!(classify_status(503), "server_error");
        assert_eq!(classify_status(700), "other");
    }

    #[test]
    fn classify_error_category_maps_auth_client_server_and_unexpected() {
        assert_eq!(classify_error_category(401), "auth_failure");
        assert_eq!(classify_error_category(403), "auth_failure");
        assert_eq!(classify_error_category(400), "client_error");
        assert_eq!(classify_error_category(404), "client_error");
        assert_eq!(classify_error_category(499), "client_error");
        assert_eq!(classify_error_category(500), "server_error");
        assert_eq!(classify_error_category(503), "server_error");
        assert_eq!(classify_error_category(599), "server_error");
        assert_eq!(classify_error_category(302), "unexpected_status");
        assert_eq!(classify_error_category(200), "unexpected_status");
    }

    #[test]
    fn metadata_identity_url_encodes_audience() {
        let url = metadata_identity_url("https://html2pdf.tsap-test.mattilsynet.io/");

        assert_eq!(
            url,
            "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/identity?audience=https%3A%2F%2Fhtml2pdf.tsap-test.mattilsynet.io%2F"
        );
    }

    #[test]
    fn renderer_error_prefix_maps_auth_client_and_server_errors() {
        assert_eq!(renderer_error_prefix(401), "html2pdf_auth_failed");
        assert_eq!(renderer_error_prefix(403), "html2pdf_auth_failed");
        assert_eq!(renderer_error_prefix(404), "html2pdf_client_error");
        assert_eq!(renderer_error_prefix(503), "html2pdf_server_error");
        assert_eq!(renderer_error_prefix(302), "html2pdf_request_failed");
    }

    #[test]
    fn safe_url_label_strips_credentials_query_and_fragment() {
        let url = "https://user:pass@renderer.example/render/pdf?token=secret#frag";

        assert_eq!(safe_url_host(url), "renderer.example");
        assert_eq!(safe_url_path(url), "/render/pdf");
        assert_eq!(safe_url_label(url), "renderer.example/render/pdf");
    }

    #[test]
    fn safe_url_label_handles_invalid_urls_without_echoing_input() {
        assert_eq!(safe_url_host("not a url with token=secret"), "unknown");
        assert_eq!(safe_url_path("not a url with token=secret"), "/");
        assert_eq!(safe_url_label("not a url with token=secret"), "unknown/");
    }

    #[test]
    fn safe_audience_label_uses_host_only() {
        assert_eq!(
            safe_audience_label("https://renderer.example/audience/path?token=secret"),
            "renderer.example"
        );
    }

    #[test]
    fn sanitize_error_message_redacts_queries_and_secrets() {
        let message = "failed url=https://renderer.example/render?token=secret Authorization: Bearer abc123 credential=abc x-api-key=def Bearer=ghi Basic=jkl";

        let sanitized = sanitize_error_message(message);

        assert!(sanitized.contains("https://renderer.example/render?redacted"));
        assert!(!sanitized.contains("secret"));
        assert!(!sanitized.contains("abc123"));
        assert!(!sanitized.contains("credential=abc"));
        assert!(!sanitized.contains("x-api-key=def"));
        assert!(!sanitized.contains("Bearer=ghi"));
        assert!(!sanitized.contains("Basic=jkl"));
        assert!(sanitized.contains("Authorization:redacted"));
        assert!(sanitized.contains("credential=redacted"));
        assert!(sanitized.contains("x-api-key=redacted"));
        assert!(sanitized.contains("Bearer=redacted"));
        assert!(sanitized.contains("Basic=redacted"));
    }

    #[test]
    fn sanitize_error_message_is_bounded() {
        let sanitized = sanitize_error_message(&"a".repeat(800));

        assert!(sanitized.len() <= ERROR_MESSAGE_CHARS + 3);
        assert!(sanitized.ends_with("..."));
    }

    #[test]
    fn renderer_feil_diagnostics_use_stable_prefixes() {
        let err = RendererFeil::recoverable(
            "html2pdf_request_failed category=timeout endpoint_host=renderer.example",
        );

        assert!(err.safe_message().starts_with("html2pdf_request_failed"));
    }

    #[test]
    fn diagnostic_format_includes_external_response_message_without_body_fields() {
        let prefix = "html2pdf_client_error";
        let category = "client_error";
        let status_code = 404;
        let status_class = "client_error";
        let endpoint_host = "renderer.example";
        let endpoint_path = "/render";
        let audience = "https://renderer.example/audience";
        let content_type = "text/html";
        let content_length = "unknown";
        let external_error_message = "suppressed";

        let diagnostic = format!(
            "{} category={} status={} status_class={} endpoint_host={} endpoint_path={} audience={} content_type=\"{}\" content_length={} external_error_message=\"{}\"",
            prefix,
            category,
            status_code,
            status_class,
            endpoint_host,
            endpoint_path,
            audience,
            content_type,
            content_length,
            external_error_message
        );

        assert!(!diagnostic.contains("body="));
        assert!(!diagnostic.contains("body_snippet"));
        assert!(!diagnostic.contains("error_body"));
        assert!(!diagnostic.contains("html_content"));
        assert!(!diagnostic.contains("pdf_content"));
        assert!(diagnostic.contains("category=client_error"));
        assert!(diagnostic.contains("status=404"));
        assert!(diagnostic.contains("status_class=client_error"));
        assert!(diagnostic.contains("endpoint_host=renderer.example"));
        assert!(diagnostic.contains("endpoint_path=/render"));
        assert!(diagnostic.contains("audience="));
        assert!(diagnostic.contains("content_type=\"text/html\""));
        assert!(diagnostic.contains("content_length=unknown"));
        assert!(diagnostic.contains("external_error_message=\"suppressed\""));
    }

    #[test]
    fn external_response_message_policy_allows_plain_text_and_json() {
        assert!(content_type_allows_external_message("text/plain"));
        assert!(content_type_allows_external_message(
            "text/plain; charset=utf-8"
        ));
        assert!(content_type_allows_external_message("application/json"));
        assert!(content_type_allows_external_message(
            "application/json; charset=utf-8"
        ));
    }

    #[test]
    fn external_response_message_policy_suppresses_html_pdf_unknown_and_auth() {
        assert!(!content_type_allows_external_message("text/html"));
        assert!(!content_type_allows_external_message("application/pdf"));
        assert!(!content_type_allows_external_message("unknown"));
        assert_eq!(
            external_response_error_message_from_text(
                401,
                "application/json",
                "credentials expired"
            ),
            None
        );
        assert_eq!(
            external_response_error_message_from_text(403, "text/plain", "forbidden"),
            None
        );
        assert_eq!(
            external_response_error_message_from_text(
                400,
                "text/html",
                "<html><body>echoed request</body></html>"
            ),
            None
        );
    }

    #[test]
    fn external_response_message_is_sanitized_bounded_and_useful() {
        let message = external_response_error_message_from_text(
            500,
            "text/plain",
            &format!(
                "renderer queue unavailable token=secret Authorization: Bearer abc123 {}",
                "a".repeat(800)
            ),
        );

        let message = message.expect("plain text response message should be logged");
        assert!(message.starts_with("renderer queue unavailable"));
        assert!(message.contains("token=redacted"));
        assert!(message.contains("Authorization:redacted"));
        assert!(!message.contains("secret"));
        assert!(!message.contains("abc123"));
        assert!(message.len() <= EXTERNAL_RESPONSE_MESSAGE_CHARS + 3);
        assert!(message.ends_with("..."));
    }

    #[tokio::test]
    async fn short_allowed_response_body_is_preserved_through_chunk_path() {
        let response = http_response(500, "text/plain", "renderer queue unavailable");

        let message = read_external_response_error_message(500, "text/plain", response).await;

        assert_eq!(message.as_deref(), Some("renderer queue unavailable"));
    }

    #[tokio::test]
    async fn html_response_body_is_suppressed_through_chunk_path() {
        let response = http_response(400, "text/html", "<html><body>echo</body></html>");

        let message = read_external_response_error_message(400, "text/html", response).await;

        assert_eq!(message, None);
    }

    fn http_response(status_code: u16, content_type: &str, body: &str) -> reqwest::Response {
        http::Response::builder()
            .status(status_code)
            .header(CONTENT_TYPE, content_type)
            .body(body.to_string())
            .expect("valid response")
            .into()
    }

    fn external_response_error_message_from_text(
        status_code: u16,
        content_type: &str,
        response_text: &str,
    ) -> Option<String> {
        if matches!(status_code, 401 | 403) || !content_type_allows_external_message(content_type) {
            return None;
        }
        let message = sanitize_external_response_message(response_text);
        (!message.is_empty()).then_some(message)
    }
}
