use std::time::Duration;

use application::command::ports::dokument_renderer_port::{DokumentRenderer, RendererFeil};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};

#[async_trait]
pub trait IdTokenProvider: Send + Sync {
    async fn id_token(&self, audience: &str) -> Result<String, RendererFeil>;
}

pub struct GcpIdTokenProvider;

#[async_trait]
impl IdTokenProvider for GcpIdTokenProvider {
    async fn id_token(&self, audience: &str) -> Result<String, RendererFeil> {
        let provider = gcp_auth::provider()
            .await
            .map_err(|_| RendererFeil::irrecoverable("Kunne ikke hente GCP token provider"))?;
        let token = provider
            .token(&[audience])
            .await
            .map_err(|_| RendererFeil::irrecoverable("Kunne ikke hente Cloud Run ID-token"))?;
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
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(60))
            .build()
            .expect("valid reqwest client config");

        Self {
            client,
            endpoint,
            audience,
            token_provider,
        }
    }
}

#[async_trait]
impl DokumentRenderer for Html2PdfRendererAdapter {
    async fn render(&self, html: &[u8]) -> Result<Vec<u8>, RendererFeil> {
        let token = self.token_provider.id_token(&self.audience).await?;
        let response = self
            .client
            .post(&self.endpoint)
            .header(CONTENT_TYPE, "text/html; charset=utf-8")
            .header(AUTHORIZATION, format!("Bearer {token}"))
            .body(html.to_vec())
            .send()
            .await
            .map_err(|_| RendererFeil::recoverable("Renderer-kall feilet midlertidig"))?;

        let status = response.status();
        if status.is_success() {
            return response
                .bytes()
                .await
                .map(|bytes| bytes.to_vec())
                .map_err(|_| RendererFeil::recoverable("Renderer-respons kunne ikke leses"));
        }

        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(RendererFeil::recoverable(
                "Renderer auth/konfigurasjon feilet; sjekk Cloud Run invoker-tilgang",
            ));
        }

        if status.is_client_error() {
            return Err(RendererFeil::irrecoverable(
                "Renderer avviste HTML-mal med klientfeil",
            ));
        }

        Err(RendererFeil::recoverable(
            "Renderer returnerte midlertidig feilstatus",
        ))
    }
}
