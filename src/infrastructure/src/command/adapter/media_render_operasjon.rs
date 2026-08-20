//! Rendring av HTML-mal til PDF.
//!
//! Skilt fra arkivgatewayen fordi den henter mal fra object store og lagrer
//! PDF, i stedet for å snakke med arkivet. Operasjonen er idempotent: PDF-en
//! lagres på en deterministisk nøkkel utledet fra `dokument_id`, så et nytt
//! forsøk overskriver samme objekt (SKU-0016 D7).

use application::command::ports::dokument_renderer_port::{
    DokumentRenderer, RendererFeil, RendererKontekst,
};
use application::command::services::eksekver_operasjon::RenderOperasjon;
use async_trait::async_trait;
use domain::eksekvering::html_template::{FeltVerdier, TemplateFelt, substituer_tokens};
use domain::eksekvering::id::SkuffenDokumentId;
use domain::eksekvering::typer::EksekveringFeil;
use uuid::Uuid;

use crate::command::media::{MediaFile, MediaMetadata, MediaStore};

/// Namespace for den deterministiske PDF-nøkkelen. Endres den, mister vi
/// idempotensen for allerede rendrede dokumenter.
const RENDERED_DOKUMENT_NAMESPACE: Uuid = uuid::uuid!("3bc0f83e-28df-5d9b-a7e2-90a6c8f2fbb4");

pub fn rendered_dokument_referanse(dokument_id: SkuffenDokumentId) -> Uuid {
    Uuid::new_v5(
        &RENDERED_DOKUMENT_NAMESPACE,
        Uuid::from(dokument_id).as_bytes(),
    )
}

pub struct MediaRenderOperasjon {
    media_store: std::sync::Arc<dyn MediaStore>,
    renderer: Box<dyn DokumentRenderer>,
}

impl MediaRenderOperasjon {
    pub fn new(
        media_store: std::sync::Arc<dyn MediaStore>,
        renderer: Box<dyn DokumentRenderer>,
    ) -> Self {
        Self {
            media_store,
            renderer,
        }
    }
}

fn feil(err: RendererFeil) -> EksekveringFeil {
    if err.is_recoverable() {
        EksekveringFeil::recoverable(err.safe_message().to_string())
    } else {
        EksekveringFeil::irrecoverable(err.safe_message().to_string())
    }
}

#[async_trait]
impl RenderOperasjon for MediaRenderOperasjon {
    async fn render(
        &self,
        dokument_id: SkuffenDokumentId,
        mal_referanse: Uuid,
        felter: &[TemplateFelt],
        saksnummer: Option<&str>,
    ) -> Result<Uuid, EksekveringFeil> {
        let mal = self
            .media_store
            .get(mal_referanse)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?
            .ok_or_else(|| {
                EksekveringFeil::irrecoverable(format!(
                    "html_mal_mangler mal_referanse={mal_referanse}"
                ))
            })?;

        let substituert = substituer_tokens(&mal.data, felter, &FeltVerdier { saksnummer })
            .map_err(|err| EksekveringFeil::irrecoverable(err.to_string()))?;

        let pdf = self
            .renderer
            .render(
                &substituert,
                RendererKontekst {
                    command_id: Uuid::nil(),
                    correlation_id: None,
                    journalpost_id: Uuid::nil().into(),
                    dokument_id,
                },
            )
            .await
            .map_err(feil)?;

        let referanse = rendered_dokument_referanse(dokument_id);
        self.media_store
            .save(MediaFile {
                id: referanse,
                data: pdf,
                filename: None,
                content_type: Some("application/pdf".to_string()),
                metadata: MediaMetadata {
                    origin: Some("skuffen-render".to_string()),
                    source_template_reference: Some(mal_referanse),
                    source_document_id: Some(Uuid::from(dokument_id)),
                    source_command_id: None,
                    render_timestamp: Some(chrono::Utc::now().to_rfc3339()),
                },
            })
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(referanse)
    }
}
