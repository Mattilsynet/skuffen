use std::time::Duration;

use application::command::ports::dokument_renderer_port::{DokumentRenderer, RendererFeil};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE};
use tracing::{debug, error, info, warn};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const ERROR_BODY_SNIPPET_BYTES: usize = 1024;
const ERROR_MESSAGE_CHARS: usize = 500;

#[async_trait]
pub trait IdTokenProvider: Send + Sync {
    async fn id_token(&self, audience: &str) -> Result<String, RendererFeil>;
}

pub struct GcpIdTokenProvider;

#[async_trait]
impl IdTokenProvider for GcpIdTokenProvider {
    async fn id_token(&self, audience: &str) -> Result<String, RendererFeil> {
        let audience_label = safe_audience_label(audience);
        let provider = gcp_auth::provider().await.map_err(|err| {
            let error_message = sanitize_error_message(&err.to_string());
            warn!(
                event = "html2pdf_auth_failed",
                category = "gcp_token_provider",
                error_class = "gcp_auth_provider",
                audience = %audience_label,
                error_message = %error_message,
                "html2pdf token provider initialization failed"
            );
            RendererFeil::irrecoverable(format!(
                "html2pdf_auth_failed category=gcp_token_provider audience={} error_class=gcp_auth_provider error_message=\"{}\"",
                audience_label,
                quote_value(&error_message)
            ))
        })?;

        let token = provider.token(&[audience]).await.map_err(|err| {
            let error_message = sanitize_error_message(&err.to_string());
            warn!(
                event = "html2pdf_auth_failed",
                category = "gcp_token_request",
                error_class = "gcp_auth_token",
                audience = %audience_label,
                error_message = %error_message,
                "html2pdf token request failed"
            );
            RendererFeil::irrecoverable(format!(
                "html2pdf_auth_failed category=gcp_token_request audience={} error_class=gcp_auth_token error_message=\"{}\"",
                audience_label,
                quote_value(&error_message)
            ))
        })?;
        Ok(token.as_str().to_string())
    }
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
    async fn render(&self, html: &[u8]) -> Result<Vec<u8>, RendererFeil> {
        let context = self.diagnostics_context();
        let html_byte_len = html.len();

        info!(
            event = "html2pdf_request_start",
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

        let body = match response.bytes().await {
            Ok(bytes) => sanitize_body_snippet(&bytes, ERROR_BODY_SNIPPET_BYTES),
            Err(err) => format!(
                "unreadable:{}",
                quote_value(&sanitize_error_message(&err.to_string()))
            ),
        };
        let body_field = format!("body=\"{}\"", quote_value(&body));

        let prefix = renderer_error_prefix(status_code);
        let diagnostic = format!(
            "{} status={} status_class={} endpoint_host={} endpoint_path={} audience={} content_type=\"{}\" content_length={} {}",
            prefix,
            status_code,
            status_class,
            context.endpoint_host,
            context.endpoint_path,
            context.audience,
            quote_value(&content_type),
            content_length,
            body_field
        );

        if status_code == 401 || status_code == 403 {
            warn!(
                event = "html2pdf_auth_failed",
                category = "auth_failure",
                status_code,
                status_class,
                content_type = %content_type,
                content_length = %content_length,
                endpoint_host = %context.endpoint_host,
                endpoint_path = %context.endpoint_path,
                audience = %context.audience,
                error_body = %body,
                "html2pdf renderer auth failure"
            );
            return Err(RendererFeil::recoverable(diagnostic));
        }

        if status.is_client_error() {
            error!(
                event = "html2pdf_client_error",
                category = "client_error",
                status_code,
                status_class,
                content_type = %content_type,
                content_length = %content_length,
                endpoint_host = %context.endpoint_host,
                endpoint_path = %context.endpoint_path,
                audience = %context.audience,
                error_body = %body,
                "html2pdf renderer returned client error"
            );
            return Err(RendererFeil::irrecoverable(diagnostic));
        }

        if status.is_server_error() {
            error!(
                event = "html2pdf_server_error",
                category = "server_error",
                status_code,
                status_class,
                content_type = %content_type,
                content_length = %content_length,
                endpoint_host = %context.endpoint_host,
                endpoint_path = %context.endpoint_path,
                audience = %context.audience,
                error_body = %body,
                "html2pdf renderer returned server error"
            );
            return Err(RendererFeil::recoverable(diagnostic));
        }

        warn!(
            event = "html2pdf_request_failed",
            category = "unexpected_status",
            status_code,
            status_class,
            content_type = %content_type,
            content_length = %content_length,
            endpoint_host = %context.endpoint_host,
            endpoint_path = %context.endpoint_path,
            audience = %context.audience,
            error_body = %body,
            "html2pdf renderer returned unexpected status"
        );
        Err(RendererFeil::recoverable(diagnostic))
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

fn sanitize_body_snippet(bytes: &[u8], max_bytes: usize) -> String {
    let limited = bytes.get(..max_bytes).unwrap_or(bytes);
    let text = String::from_utf8_lossy(limited);
    let normalized = normalize_control_chars(&text);
    let redacted = redact_sensitive_tokens(&strip_url_queries(&normalized));
    if bytes.len() > max_bytes {
        truncate_chars(&redacted, max_bytes)
    } else {
        redacted
    }
}

fn sanitize_error_message(message: &str) -> String {
    let normalized = normalize_control_chars(message);
    let without_queries = strip_url_queries(&normalized);
    let redacted = redact_sensitive_tokens(&without_queries);
    truncate_chars(redacted.trim(), ERROR_MESSAGE_CHARS)
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
        || lower == "basic"
        || lower.contains("password")
        || lower.contains("passwd")
        || lower.contains("token")
        || lower.contains("api_key")
        || lower.contains("apikey")
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
    fn body_snippet_is_bounded_and_sanitized() {
        let body = format!(
            "forbidden\nAuthorization: Bearer abc123 token=secret {}",
            "a".repeat(1200)
        );

        let snippet = sanitize_body_snippet(body.as_bytes(), ERROR_BODY_SNIPPET_BYTES);

        assert!(snippet.len() <= ERROR_BODY_SNIPPET_BYTES + 3);
        assert!(snippet.ends_with("..."));
        assert!(!snippet.contains("abc123"));
        assert!(!snippet.contains("secret"));
        assert!(!snippet.contains('\n'));
    }

    #[test]
    fn body_snippet_replaces_invalid_utf8() {
        let snippet = sanitize_body_snippet(b"bad\xff\xfe body", ERROR_BODY_SNIPPET_BYTES);

        assert!(snippet.contains("bad"));
        assert!(snippet.contains("body"));
    }

    #[test]
    fn sanitize_error_message_redacts_queries_and_secrets() {
        let message =
            "failed url=https://renderer.example/render?token=secret Authorization: Bearer abc123";

        let sanitized = sanitize_error_message(message);

        assert!(sanitized.contains("https://renderer.example/render?redacted"));
        assert!(!sanitized.contains("secret"));
        assert!(!sanitized.contains("abc123"));
        assert!(sanitized.contains("Authorization:redacted"));
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
}
