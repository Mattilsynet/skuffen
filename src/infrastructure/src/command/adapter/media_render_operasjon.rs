//! Rendring av HTML-mal til PDF.
//!
//! PDF-en lagres på en deterministisk nøkkel utledet fra `dokument_id`, så et
//! nytt forsøk overskriver samme objekt (SKU-0016 D7).

use application::command::ports::dokument_renderer_port::{
    DokumentRenderer, RendererFeil, RendererKontekst,
};
use application::command::services::eksekver_operasjon::RenderOperasjon;
use async_trait::async_trait;
use domain::eksekvering::html_template::{FeltVerdier, TemplateFelt, substituer_tokens};
use domain::eksekvering::id::SkuffenDokumentId;
use domain::eksekvering::typer::{EksekveringFeil, StatusErrorCode};
use uuid::Uuid;

use crate::command::media::{MediaFile, MediaMetadata, MediaStore};

/// Endres denne, mister allerede rendrede dokumenter idempotensen sin.
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

/// Rendreren har allerede klassifisert feilen og gitt den en trygg tekst;
/// her legges kun kode og klientvendt feilkode på.
fn feil(err: RendererFeil) -> EksekveringFeil {
    if err.is_recoverable() {
        EksekveringFeil::recoverable(
            "render_utilgjengelig",
            "Dokumentproduksjonen er midlertidig utilgjengelig. Prøv igjen senere.",
            StatusErrorCode::TemporaryUnavailable,
        )
        .med_intern_detalj(err.safe_message().to_string())
    } else {
        EksekveringFeil::irrecoverable(
            "render_avvist",
            "Dokumentet kunne ikke produseres fra malen.",
            StatusErrorCode::InvalidRequest,
        )
        .med_intern_detalj(err.safe_message().to_string())
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
            .map_err(|err| {
                EksekveringFeil::intern_midlertidig("intern_mal_utilgjengelig")
                    .med_intern_detalj(err.to_string())
            })?
            .ok_or_else(|| {
                EksekveringFeil::irrecoverable(
                    "render_mal_mangler",
                    "Malen det vises til finnes ikke.",
                    StatusErrorCode::InvalidRequest,
                )
                .med_intern_detalj(format!("mal_referanse={mal_referanse}"))
            })?;

        let substituert = substituer_tokens(&mal.data, felter, &FeltVerdier { saksnummer })
            .map_err(|err| {
                EksekveringFeil::irrecoverable(
                    "render_mal_substitusjon_feilet",
                    "Malen kunne ikke fylles ut med de oppgitte feltene.",
                    StatusErrorCode::InvalidRequest,
                )
                .med_intern_detalj(err.to_string())
            })?;

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
            .map_err(|err| {
                EksekveringFeil::intern_midlertidig("intern_lagring_av_rendret_dokument_feilet")
                    .med_intern_detalj(err.to_string())
            })?;

        Ok(referanse)
    }
}
