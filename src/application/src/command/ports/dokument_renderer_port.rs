use async_trait::async_trait;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RendererFeil {
    #[error("renderer er midlertidig utilgjengelig")]
    Recoverable { message: String },
    #[error("renderer avviste dokumentet")]
    Irrecoverable { message: String },
}

impl RendererFeil {
    pub fn recoverable(message: impl Into<String>) -> Self {
        Self::Recoverable {
            message: message.into(),
        }
    }

    pub fn irrecoverable(message: impl Into<String>) -> Self {
        Self::Irrecoverable {
            message: message.into(),
        }
    }

    pub fn safe_message(&self) -> &str {
        match self {
            RendererFeil::Recoverable { message } | RendererFeil::Irrecoverable { message } => {
                message
            }
        }
    }

    pub fn is_recoverable(&self) -> bool {
        matches!(self, RendererFeil::Recoverable { .. })
    }
}

#[async_trait]
pub trait DokumentRenderer: Send + Sync {
    async fn render(&self, html: &[u8]) -> Result<Vec<u8>, RendererFeil>;
}

#[derive(Clone)]
pub struct IkkeKonfigurertDokumentRenderer;

#[async_trait]
impl DokumentRenderer for IkkeKonfigurertDokumentRenderer {
    async fn render(&self, _html: &[u8]) -> Result<Vec<u8>, RendererFeil> {
        Err(RendererFeil::recoverable(
            "HTML-template rendering er ikke konfigurert",
        ))
    }
}

#[cfg(test)]
pub mod fake {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::{DokumentRenderer, RendererFeil};
    use async_trait::async_trait;

    type RenderOutcome = Result<Vec<u8>, RendererFeil>;
    type ScriptedRenderOutcomes = VecDeque<RenderOutcome>;

    #[derive(Clone)]
    pub struct FakeDokumentRenderer {
        outcomes: Arc<Mutex<ScriptedRenderOutcomes>>,
    }

    impl FakeDokumentRenderer {
        pub fn success(pdf: Vec<u8>) -> Self {
            Self::scripted(vec![Ok(pdf)])
        }

        pub fn scripted(outcomes: Vec<RenderOutcome>) -> Self {
            Self {
                outcomes: Arc::new(Mutex::new(outcomes.into())),
            }
        }
    }

    #[async_trait]
    impl DokumentRenderer for FakeDokumentRenderer {
        async fn render(&self, _html: &[u8]) -> Result<Vec<u8>, RendererFeil> {
            self.outcomes
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(b"%PDF fake".to_vec()))
        }
    }
}
