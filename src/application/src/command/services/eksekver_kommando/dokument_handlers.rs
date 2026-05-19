use crate::command::ports::dokument_lager_port::{DokumentFil, DokumentMetadata};
use domain::eksekvering::html_template::{substituer_tokens, FeltVerdier, HtmlTemplateFeil};
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId};
use domain::eksekvering::tilstand::{
    DokumentKildeTilstand, DokumentMedTilstand, DokumentTilstand, SakMedBarn,
};
use domain::eksekvering::typer::{EksekveringFeil, EksekveringFeiltype};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use uuid::Uuid;

use super::{extract_dokument_client_references, EksekverKommandoService};

impl EksekverKommandoService {
    pub(super) async fn render_dokument(
        &self,
        envelope: &CommandEnvelope<Command>,
        sak: &SakMedBarn,
        journalpost_id: SkuffenJournalpostId,
        dokument_id: SkuffenDokumentId,
    ) -> Result<(), EksekveringFeil> {
        let dokument = finn_dokument(sak, journalpost_id, dokument_id)?;
        if dokument.tilstand == DokumentTilstand::Ok {
            return Ok(());
        }

        let DokumentKildeTilstand::HtmlTemplate {
            mal_referanse,
            felter,
            rendered_dokument_referanse,
        } = &dokument.kilde
        else {
            return Err(EksekveringFeil::irrecoverable(format!(
                "render_ikke_html_template dokument_id={} journalpost_id={}",
                dokument_id.0, journalpost_id.0
            )));
        };

        if rendered_dokument_referanse.is_some() {
            self.entity_tilstand_repo
                .oppdater_dokument_tilstand(dokument_id, DokumentTilstand::Ok)
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
            return Ok(());
        }

        let saksnummer = sak
            .saksnummer
            .as_deref()
            .ok_or_else(|| EksekveringFeil::blocked("render_saksnummer_mangler"))?;

        let html = match self.dokument_lager.get(*mal_referanse).await {
            Ok(Some(media)) => media.data,
            Ok(None) => {
                return Err(EksekveringFeil::recoverable(format!(
                    "render_html_mal_mangler mal_referanse={mal_referanse}"
                )));
            }
            Err(err) => {
                return Err(EksekveringFeil::recoverable(format!(
                    "render_html_mal_lager_unavailable mal_referanse={} error=\"{}\"",
                    mal_referanse,
                    quote_diagnostic_value(&sanitize_render_error(&err.to_string()))
                )));
            }
        };

        let substituert = match substituer_tokens(
            &html,
            felter,
            &FeltVerdier {
                saksnummer: Some(saksnummer),
            },
        ) {
            Ok(html) => html,
            Err(err) => {
                let melding = template_feil_melding(&err);
                let diagnostic = format!(
                    "render_token_substitution_failed reason={}",
                    template_feil_kode(&err)
                );
                self.entity_tilstand_repo
                    .oppdater_dokument_tilstand(dokument_id, DokumentTilstand::FeiletPermanent)
                    .await
                    .map_err(|repo_err| {
                        render_state_update_failed("oppdater_dokument_tilstand", repo_err)
                    })?;
                self.entity_tilstand_repo
                    .logg_overgang(
                        "dokument",
                        dokument_id.0,
                        envelope.command_id,
                        "avventer_rendring",
                        "feilet_permanent",
                        "render_dokument",
                        Some(melding),
                    )
                    .await
                    .map_err(|repo_err| render_state_update_failed("logg_overgang", repo_err))?;
                return Err(EksekveringFeil::irrecoverable(diagnostic));
            }
        };

        let pdf = self
            .dokument_renderer
            .render(&substituert)
            .await
            .map_err(|err| {
                if err.is_recoverable() {
                    EksekveringFeil::recoverable(err.safe_message().to_string())
                } else {
                    EksekveringFeil::irrecoverable(err.safe_message().to_string())
                }
            })?;

        let rendered_id = rendered_dokument_id(dokument_id);
        self.dokument_lager
            .save(DokumentFil {
                id: rendered_id,
                data: pdf,
                filename: Some(format!("{rendered_id}.pdf")),
                content_type: Some("application/pdf".to_string()),
                metadata: DokumentMetadata {
                    origin: Some("skuffen_html_template_renderer".to_string()),
                    source_template_reference: Some(*mal_referanse),
                    source_document_id: Some(dokument_id.0),
                    source_command_id: Some(envelope.command_id),
                    render_timestamp: Some(chrono::Utc::now().to_rfc3339()),
                },
            })
            .await
            .map_err(|err| {
                EksekveringFeil::recoverable(format!(
                    "rendered_dokument_save_failed rendered_id={} error=\"{}\"",
                    rendered_id,
                    quote_diagnostic_value(&sanitize_render_error(&err.to_string()))
                ))
            })?;

        self.entity_tilstand_repo
            .oppdater_rendered_dokument_referanse(dokument_id, rendered_id)
            .await
            .map_err(|err| {
                render_state_update_failed("oppdater_rendered_dokument_referanse", err)
            })?;

        self.entity_tilstand_repo
            .oppdater_dokument_tilstand(dokument_id, DokumentTilstand::Ok)
            .await
            .map_err(|err| render_state_update_failed("oppdater_dokument_tilstand", err))?;

        self.entity_tilstand_repo
            .logg_overgang(
                "dokument",
                dokument_id.0,
                envelope.command_id,
                "avventer_rendring",
                "ok",
                "render_dokument",
                None,
            )
            .await
            .map_err(|err| render_state_update_failed("logg_overgang", err))?;

        Ok(())
    }

    pub(super) async fn legg_til_dokument(
        &self,
        envelope: &CommandEnvelope<Command>,
        sak: &SakMedBarn,
        journalpost_id: SkuffenJournalpostId,
        dokument_id: SkuffenDokumentId,
    ) -> Result<(), EksekveringFeil> {
        let jp = sak
            .journalposter
            .iter()
            .find(|jp| jp.journalpost_id == journalpost_id)
            .ok_or_else(|| {
                EksekveringFeil::recoverable(format!(
                    "Fant ikke journalpost {} i sak {}",
                    journalpost_id.0, sak.sak_id.0
                ))
            })?;

        let journalpostnummer = jp.journalpostnummer.ok_or_else(|| {
            EksekveringFeil::blocked("Journalpostnummer mangler for legg_til_dokument")
        })?;

        let dokument_client_reference = self
            .resolve_dokument_client_reference(envelope, dokument_id)
            .await?;

        let resp = match self
            .arkiv_gateway
            .legg_til_vedlegg(envelope, journalpostnummer, vec![dokument_client_reference])
            .await
        {
            Ok(resp) => resp,
            Err(err) => {
                let err = self.map_arkiv_feil(err);
                if matches!(err.feiltype, EksekveringFeiltype::Irrecoverable) {
                    let _ = self
                        .entity_tilstand_repo
                        .oppdater_dokument_tilstand(dokument_id, DokumentTilstand::FeiletPermanent)
                        .await;

                    let _ = self
                        .entity_tilstand_repo
                        .logg_overgang(
                            "dokument",
                            dokument_id.0,
                            envelope.command_id,
                            "ikke_realisert",
                            "feilet_permanent",
                            "legg_til_dokument",
                            Some(&err.melding),
                        )
                        .await;
                }
                return Err(err);
            }
        };

        if let Some(Some(arkiv_id)) = resp.into_iter().next() {
            self.id_mapping
                .oppdater_arkiv_id_for_client_reference(
                    dokument_client_reference,
                    arkiv_id.to_string(),
                )
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        }

        self.entity_tilstand_repo
            .oppdater_dokument_tilstand(dokument_id, DokumentTilstand::Ok)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        self.entity_tilstand_repo
            .logg_overgang(
                "dokument",
                dokument_id.0,
                envelope.command_id,
                "ikke_realisert",
                "ok",
                "legg_til_dokument",
                None,
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(())
    }

    async fn resolve_dokument_client_reference(
        &self,
        envelope: &CommandEnvelope<Command>,
        dokument_id: SkuffenDokumentId,
    ) -> Result<Uuid, EksekveringFeil> {
        let client_references = extract_dokument_client_references(envelope);
        for client_ref in client_references {
            let resolved = self
                .id_mapping
                .hent_dokument_id_fra_mapping(client_ref)
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
            if resolved == Some(dokument_id) {
                return Ok(client_ref);
            }
        }

        Err(EksekveringFeil::recoverable(format!(
            "Fant ikke client_reference for dokument {}",
            dokument_id.0
        )))
    }
}

fn finn_dokument(
    sak: &SakMedBarn,
    journalpost_id: SkuffenJournalpostId,
    dokument_id: SkuffenDokumentId,
) -> Result<&DokumentMedTilstand, EksekveringFeil> {
    sak.journalposter
        .iter()
        .find(|jp| jp.journalpost_id == journalpost_id)
        .and_then(|jp| {
            jp.dokumenter
                .iter()
                .find(|dok| dok.dokument_id == dokument_id)
        })
        .ok_or_else(|| {
            let journalpost_exists = sak
                .journalposter
                .iter()
                .any(|jp| jp.journalpost_id == journalpost_id);
            if journalpost_exists {
                EksekveringFeil::recoverable(format!(
                    "render_dokument_mangler dokument_id={} journalpost_id={}",
                    dokument_id.0, journalpost_id.0
                ))
            } else {
                EksekveringFeil::recoverable(format!(
                    "render_journalpost_mangler journalpost_id={} dokument_id={}",
                    journalpost_id.0, dokument_id.0
                ))
            }
        })
}

fn template_feil_melding(err: &HtmlTemplateFeil) -> &'static str {
    match err {
        HtmlTemplateFeil::ForStor => "HTML-mal er for stor",
        HtmlTemplateFeil::UgyldigUtf8 => "HTML-mal er ikke gyldig UTF-8",
        HtmlTemplateFeil::UkjentToken => "HTML-mal inneholder ukjent token",
        HtmlTemplateFeil::ManglerToken => "HTML-mal mangler deklarert token",
        HtmlTemplateFeil::DuplikatToken => "HTML-mal inneholder duplikat token",
        HtmlTemplateFeil::DuplikatFelt => "Deklarerte felter inneholder duplikat",
        HtmlTemplateFeil::TommeFelter => "Deklarerte felter kan ikke være tomme",
        HtmlTemplateFeil::ManglerSaksnummer => "Saksnummer mangler",
    }
}

fn template_feil_kode(err: &HtmlTemplateFeil) -> &'static str {
    match err {
        HtmlTemplateFeil::ForStor => "for_stor",
        HtmlTemplateFeil::UgyldigUtf8 => "ugyldig_utf8",
        HtmlTemplateFeil::UkjentToken => "ukjent_token",
        HtmlTemplateFeil::ManglerToken => "mangler_token",
        HtmlTemplateFeil::DuplikatToken => "duplikat_token",
        HtmlTemplateFeil::DuplikatFelt => "duplikat_felt",
        HtmlTemplateFeil::TommeFelter => "tomme_felter",
        HtmlTemplateFeil::ManglerSaksnummer => "mangler_saksnummer",
    }
}

fn render_state_update_failed(operation: &'static str, err: anyhow::Error) -> EksekveringFeil {
    EksekveringFeil::recoverable(format!(
        "render_state_update_failed operation={} error=\"{}\"",
        operation,
        quote_diagnostic_value(&sanitize_render_error(&err.to_string()))
    ))
}

fn sanitize_render_error(detail: &str) -> String {
    let normalized = detail
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let redacted = redact_diagnostic_tokens(&normalized);

    const MAX_RENDER_ERROR_DETAIL: usize = 240;
    if redacted.chars().count() <= MAX_RENDER_ERROR_DETAIL {
        redacted
    } else {
        format!(
            "{}…",
            redacted
                .chars()
                .take(MAX_RENDER_ERROR_DETAIL)
                .collect::<String>()
        )
    }
}

fn redact_diagnostic_tokens(detail: &str) -> String {
    let mut redacted = Vec::new();
    let mut redact_next = 0;
    for token in detail.split_whitespace() {
        if redact_next > 0 {
            redacted.push("redacted".to_string());
            redact_next -= 1;
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if is_sensitive_diagnostic_token(&lower) {
            redacted.push(redact_diagnostic_token(token));
            redact_next = sensitive_following_token_count(token, &lower);
        } else {
            redacted.push(token.to_string());
        }
    }
    redacted.join(" ")
}

fn is_sensitive_diagnostic_token(lower: &str) -> bool {
    lower.contains("authorization")
        || lower == "bearer"
        || lower.starts_with("bearer=")
        || lower == "basic"
        || lower.starts_with("basic=")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("passwd")
        || lower.contains("credential")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("x-api-key")
}

fn sensitive_following_token_count(token: &str, lower: &str) -> usize {
    if lower.contains("authorization") && token.ends_with(':') {
        2
    } else if token.ends_with(':') || lower == "bearer" || lower == "basic" {
        1
    } else {
        0
    }
}

fn redact_diagnostic_token(token: &str) -> String {
    if let Some((key, _)) = token.split_once('=') {
        format!("{key}=redacted")
    } else if let Some((key, _)) = token.split_once(':') {
        format!("{key}:redacted")
    } else {
        "redacted".to_string()
    }
}

fn quote_diagnostic_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn rendered_dokument_id(dokument_id: SkuffenDokumentId) -> Uuid {
    const RENDERED_DOKUMENT_NAMESPACE: Uuid = uuid::uuid!("3bc0f83e-28df-5d9b-a7e2-90a6c8f2fbb4");
    Uuid::new_v5(&RENDERED_DOKUMENT_NAMESPACE, dokument_id.0.as_bytes())
}
