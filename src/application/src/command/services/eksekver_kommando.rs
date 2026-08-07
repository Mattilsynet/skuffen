mod dokument_handlers;
mod journalpost_handlers;
mod lifecycle_publisher;
mod sak_handlers;

use crate::command::{Command as ApplicationCommand, CommandEnvelope, SakKey};
use anyhow::Context;
use domain::command::Command as DomainCommand;
use domain::eksekvering::id::{SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::{CommandStateDecision, SakMedBarn, planlegg_neste_handling};
use domain::eksekvering::typer::EksekveringFeil;
use uuid::Uuid;

use crate::command::ports::dokument_lager_port::DokumentLager;
use crate::command::ports::dokument_renderer_port::DokumentRenderer;
use crate::command::ports::eksekvering_port::{
    ArkivGateway, EksekveringKvitteringPublisher, EksekveringStatusPublisher,
};
use crate::command::ports::entity_tilstand_port::EntityTilstandRepository;
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::ports::ventende_kommando_wakeup_port::VentendeKommandoWakeup;
use crate::command::services::command_state_decision::{blocked_detail, invalid_detail};

pub trait IntoExecutorEnvelope {
    fn into_executor_envelope(self) -> CommandEnvelope<ApplicationCommand>;
}

impl IntoExecutorEnvelope for CommandEnvelope<ApplicationCommand> {
    fn into_executor_envelope(self) -> CommandEnvelope<ApplicationCommand> {
        self
    }
}

pub struct EksekverKommandoService {
    entity_tilstand_repo: Box<dyn EntityTilstandRepository>,
    arkiv_gateway: Box<dyn ArkivGateway>,
    dokument_renderer: Box<dyn DokumentRenderer>,
    dokument_lager: Box<dyn DokumentLager>,
    id_mapping: Box<dyn IdMappingRepository>,
    status_publisher: Box<dyn EksekveringStatusPublisher>,
    done_publisher: Box<dyn EksekveringKvitteringPublisher>,
    outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
    wakeup_service: Box<dyn VentendeKommandoWakeup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionOutcome {
    Klar,
    Ok,
    BlokkertVenter { last_error: Option<String> },
    Retrying { last_error: Option<String> },
    Feil { last_error: Option<String> },
}

impl EksekverKommandoService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        entity_tilstand_repo: Box<dyn EntityTilstandRepository>,
        arkiv_gateway: Box<dyn ArkivGateway>,
        dokument_renderer: Box<dyn DokumentRenderer>,
        dokument_lager: Box<dyn DokumentLager>,
        status_publisher: Box<dyn EksekveringStatusPublisher>,
        done_publisher: Box<dyn EksekveringKvitteringPublisher>,
        id_mapping: Box<dyn IdMappingRepository>,
        outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
        wakeup_service: Box<dyn VentendeKommandoWakeup>,
    ) -> Self {
        Self {
            entity_tilstand_repo,
            arkiv_gateway,
            dokument_renderer,
            dokument_lager,
            id_mapping,
            status_publisher,
            done_publisher,
            outward_status_projector,
            wakeup_service,
        }
    }

    pub async fn handle(
        &self,
        envelope: impl IntoExecutorEnvelope,
        attempt: u32,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        self.handle_internal(envelope.into_executor_envelope(), attempt)
            .await
    }

    async fn handle_internal(
        &self,
        envelope: CommandEnvelope<ApplicationCommand>,
        attempt: u32,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let domain_command = self.resolve_domain_command_for_envelope(&envelope).await?;
        let sak_id = domain_command.sak_id();

        let sak_med_barn = self.hent_sak_med_barn(sak_id).await?;
        match planlegg_neste_handling(&domain_command, &sak_med_barn) {
            CommandStateDecision::Ready(operasjon) => {
                match self
                    .utfoer_operasjon(&envelope, &sak_med_barn, operasjon)
                    .await
                {
                    Ok(()) => {
                        let _ = self.wakeup_after_operation(sak_id, operasjon).await;
                        let oppdatert_sak_med_barn = self.hent_sak_med_barn(sak_id).await?;
                        //TODO: Trenger dette å være en nested loop, eller kan vi ha en mer generell
                        //loop som dekker begge?
                        let neste_beslutning =
                            planlegg_neste_handling(&domain_command, &oppdatert_sak_med_barn);
                        match neste_beslutning {
                            CommandStateDecision::Done => {
                                self.publish_success(&envelope, attempt).await
                            }
                            CommandStateDecision::Ready(_)
                            | CommandStateDecision::Blocked(_)
                            | CommandStateDecision::Invalid(_) => Ok(ExecutionOutcome::Klar),
                        }
                    }
                    Err(feil) => {
                        let _ = self.wakeup_after_operation(sak_id, operasjon).await;
                        self.map_feil_til_outcome(&envelope, feil, attempt).await
                    }
                }
            }
            beslutning => {
                self.materialiser_beslutning(&envelope, attempt, beslutning)
                    .await
            }
        }
    }

    async fn resolve_domain_command_for_envelope(
        &self,
        envelope: &CommandEnvelope<ApplicationCommand>,
    ) -> Result<DomainCommand, anyhow::Error> {
        let sak_id = self.resolve_sak_id_for_envelope(envelope).await?;

        match &envelope.payload {
            ApplicationCommand::OpprettSak(_) => Ok(DomainCommand::OpprettSak { sak_id }),
            ApplicationCommand::AvsluttSak(_) => Ok(DomainCommand::AvsluttSak { sak_id }),
            ApplicationCommand::SettSaksansvarlig(_) => {
                Ok(DomainCommand::SettSaksansvarlig { sak_id })
            }
            ApplicationCommand::OpprettInngaaendeJournalpost(_) => {
                Ok(DomainCommand::OpprettInngaaendeJournalpost {
                    sak_id,
                    journalpost_id: self.resolve_journalpost_id_for_envelope(envelope).await?,
                })
            }
            ApplicationCommand::OpprettUtgaaendeJournalpost(_) => {
                Ok(DomainCommand::OpprettUtgaaendeJournalpost {
                    sak_id,
                    journalpost_id: self.resolve_journalpost_id_for_envelope(envelope).await?,
                })
            }
            ApplicationCommand::OpprettInterntNotatJournalpost(_) => {
                Ok(DomainCommand::OpprettInterntNotatJournalpost {
                    sak_id,
                    journalpost_id: self.resolve_journalpost_id_for_envelope(envelope).await?,
                })
            }
        }
    }

    async fn hent_sak_med_barn(&self, sak_id: SkuffenSakId) -> Result<SakMedBarn, anyhow::Error> {
        self.entity_tilstand_repo
            .hent_sak_med_barn(sak_id)
            .await
            .context("Feil ved henting av sak med barn")?
            .ok_or_else(|| anyhow::anyhow!("Sak {} finnes ikke i tilstandstabeller", sak_id.0))
    }

    async fn materialiser_beslutning(
        &self,
        envelope: &CommandEnvelope<ApplicationCommand>,
        attempt: u32,
        beslutning: CommandStateDecision,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        match beslutning {
            CommandStateDecision::Ready(_) => Ok(ExecutionOutcome::Klar),
            CommandStateDecision::Blocked(reason) => {
                //TODO: Separer publish og logikk.
                //Her skal det være publish, og return Ok(ExectuionOutcome)
                self.publish_blocked_with_detail(envelope, attempt, blocked_detail(reason))
                    .await
            }
            CommandStateDecision::Done => self.publish_success(envelope, attempt).await,
            CommandStateDecision::Invalid(violation) => {
                self.map_feil_til_outcome(
                    envelope,
                    EksekveringFeil::irrecoverable(invalid_detail(violation)),
                    attempt,
                )
                .await
            }
        }
    }

    async fn wakeup_after_operation(
        &self,
        sak_id: SkuffenSakId,
        operasjon: domain::eksekvering::tilstand::ArkivOperasjon,
    ) -> Result<(), anyhow::Error> {
        use domain::eksekvering::tilstand::ArkivOperasjon;

        match operasjon {
            ArkivOperasjon::OpprettSak { .. }
            | ArkivOperasjon::AvsluttSak { .. }
            | ArkivOperasjon::SettSaksansvarlig { .. } => {
                self.wakeup_service.etter_sak_endret(sak_id).await
            }
            ArkivOperasjon::OpprettJournalpost { journalpost_id }
            | ArkivOperasjon::Journalfoer { journalpost_id }
            | ArkivOperasjon::Avskriv { journalpost_id } => {
                self.wakeup_service
                    .etter_journalpost_endret(journalpost_id)
                    .await
            }
            ArkivOperasjon::LeggTilDokument { dokument_id, .. }
            | ArkivOperasjon::RenderDokument { dokument_id, .. } => {
                self.wakeup_service.etter_dokument_endret(dokument_id).await
            }
        }
    }

    async fn utfoer_operasjon(
        &self,
        envelope: &CommandEnvelope<ApplicationCommand>,
        sak: &SakMedBarn,
        operasjon: domain::eksekvering::tilstand::ArkivOperasjon,
    ) -> Result<(), EksekveringFeil> {
        use domain::eksekvering::tilstand::ArkivOperasjon;

        match operasjon {
            ArkivOperasjon::OpprettSak { sak_id } => self.opprett_sak(envelope, sak_id).await,
            ArkivOperasjon::OpprettJournalpost { journalpost_id } => {
                self.opprett_journalpost(envelope, sak, journalpost_id)
                    .await
            }
            ArkivOperasjon::LeggTilDokument {
                journalpost_id,
                dokument_id,
            } => {
                self.legg_til_dokument(envelope, sak, journalpost_id, dokument_id)
                    .await
            }
            ArkivOperasjon::RenderDokument {
                journalpost_id,
                dokument_id,
            } => {
                self.render_dokument(envelope, sak, journalpost_id, dokument_id)
                    .await
            }
            ArkivOperasjon::Journalfoer { journalpost_id } => {
                self.journalfoer(envelope, sak, journalpost_id).await
            }
            ArkivOperasjon::Avskriv { journalpost_id } => {
                self.avskriv(envelope, sak, journalpost_id).await
            }
            ArkivOperasjon::AvsluttSak { sak_id: _ } => self.avslutt_sak(envelope, sak).await,
            ArkivOperasjon::SettSaksansvarlig { sak_id: _ } => {
                self.sett_saksansvarlig(envelope, sak).await
            }
        }
    }
    async fn resolve_sak_id_for_envelope(
        &self,
        envelope: &CommandEnvelope<ApplicationCommand>,
    ) -> Result<SkuffenSakId, anyhow::Error> {
        let sak_key = extract_sak_key(envelope)?;
        match sak_key {
            SakKey::ClientReference(client_reference) => self
                .id_mapping
                .hent_sak_id_fra_mapping(client_reference)
                .await
                .context("Feil ved oppslag av sak_id fra client_reference")?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Fant ikke skuffen_id for sak client_reference {}",
                        client_reference
                    )
                }),
            SakKey::ArkivId(saksnummer) => self
                .id_mapping
                .hent_sak_id_fra_arkiv_id_i_mapping(saksnummer.as_str())
                .await
                .context("Feil ved oppslag av sak_id fra arkiv_id")?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Fant ikke skuffen_id for sak arkiv_id {}",
                        saksnummer.as_str()
                    )
                }),
        }
    }

    async fn resolve_journalpost_id_for_envelope(
        &self,
        envelope: &CommandEnvelope<ApplicationCommand>,
    ) -> Result<SkuffenJournalpostId, anyhow::Error> {
        let client_reference = extract_journalpost_client_reference(envelope).ok_or_else(|| {
            anyhow::anyhow!("Mangler journalpost client_reference for journalpost-kommando")
        })?;

        self.id_mapping
            .hent_journalpost_id_fra_mapping(client_reference)
            .await
            .context("Feil ved oppslag av journalpost_id fra client_reference")?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Fant ikke skuffen_id for journalpost client_reference {}",
                    client_reference
                )
            })
    }

    fn map_arkiv_feil(&self, err: anyhow::Error) -> EksekveringFeil {
        let original = err.to_string();
        let message = safe_execution_detail(&original);

        map_arkiv_feil_detail(&original, message)
    }
}

fn map_arkiv_feil_detail(original: &str, message: String) -> EksekveringFeil {
    if original.contains("sikri_recoverability=irrecoverable") {
        return EksekveringFeil::irrecoverable(message);
    }

    EksekveringFeil::recoverable(message)
}

fn safe_execution_detail(detail: &str) -> String {
    let stripped = detail
        .replace("sikri_recoverability=irrecoverable", "")
        .replace("sikri_recoverability=recoverable", "");
    let normalized = normalize_execution_detail(&stripped);

    if let Some(code) = normalized
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .find(|token| token.starts_with("sikri_"))
    {
        return code.to_string();
    }

    if let Some(diagnostic_detail) = safe_internal_execution_detail(&normalized) {
        return diagnostic_detail;
    }

    if detail.contains("sikri_recoverability=") {
        return "execution_upstream_error".to_string();
    }

    "execution_error".to_string()
}

fn safe_internal_execution_detail(detail: &str) -> Option<String> {
    const PREFIXES: [&str; 18] = [
        "blocked_reason=",
        "html2pdf_auth_failed",
        "html2pdf_client_error",
        "html2pdf_server_error",
        "html2pdf_request_failed",
        "html2pdf_response_read_failed",
        "render_dokument_mangler",
        "render_journalpost_mangler",
        "render_ikke_html_template",
        "render_saksnummer_mangler",
        "render_html_mal_mangler",
        "render_html_mal_lager_unavailable",
        "render_token_substitution_failed",
        "rendered_dokument_save_failed",
        "render_state_update_failed",
        "arkivmapping_dokument_fact_mangler",
        "arkivmapping_rendered_dokument_mangler",
        "arkivmapping_dokumentform_mismatch",
    ];

    let (start, prefix) = PREFIXES
        .iter()
        .filter_map(|prefix| detail.find(prefix).map(|index| (index, *prefix)))
        .min_by_key(|(index, _)| *index)?;

    let candidate = strip_embedded_execution_payload(&detail[start..]);
    let candidate = if prefix.starts_with("html2pdf_") {
        strip_renderer_body_fields(&candidate)
    } else {
        candidate
    };
    let redacted = redact_sensitive_execution_tokens(&candidate);
    let bounded = truncate_execution_detail(redacted.trim(), 500);

    (!bounded.is_empty()).then_some(bounded)
}

fn normalize_execution_detail(detail: &str) -> String {
    detail
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_sensitive_execution_tokens(detail: &str) -> String {
    let mut redacted = Vec::new();
    let mut redact_next = 0;

    for token in detail.split_whitespace() {
        if redact_next > 0 {
            redacted.push("redacted".to_string());
            redact_next -= 1;
            continue;
        }

        let lower = token.to_ascii_lowercase();
        if is_sensitive_execution_token(&lower) {
            redacted.push(redact_sensitive_execution_token(token));
            redact_next = sensitive_execution_following_token_count(token, &lower);
        } else {
            redacted.push(token.to_string());
        }
    }

    redacted.join(" ")
}

fn is_sensitive_execution_token(lower: &str) -> bool {
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

fn sensitive_execution_following_token_count(token: &str, lower: &str) -> usize {
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

fn redact_sensitive_execution_token(token: &str) -> String {
    token
        .find(['=', ':'])
        .map(|index| format!("{}redacted", &token[..=index]))
        .unwrap_or_else(|| "redacted".to_string())
}

fn strip_embedded_execution_payload(detail: &str) -> String {
    let Some(index) = detail.find('{') else {
        return detail.to_string();
    };

    let prefix = detail[..index].trim_end();
    if prefix.is_empty() {
        "[payload stripped]".to_string()
    } else {
        format!("{prefix} [payload stripped]")
    }
}

fn strip_renderer_body_fields(detail: &str) -> String {
    let mut stripped = String::new();
    let mut index = 0;

    while index < detail.len() {
        if starts_renderer_body_field(detail, index, "body") {
            index = skip_renderer_body_field(detail, index, "body");
            continue;
        }

        if starts_renderer_body_field(detail, index, "error_body") {
            index = skip_renderer_body_field(detail, index, "error_body");
            continue;
        }

        let ch = detail[index..]
            .chars()
            .next()
            .expect("index is within a char boundary");
        stripped.push(ch);
        index += ch.len_utf8();
    }

    stripped.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn starts_renderer_body_field(detail: &str, index: usize, field: &str) -> bool {
    if index > 0 {
        let Some(previous) = detail[..index].chars().next_back() else {
            return false;
        };
        if !previous.is_whitespace() {
            return false;
        }
    }

    detail[index..].starts_with(field) && detail[index + field.len()..].starts_with('=')
}

fn skip_renderer_body_field(detail: &str, index: usize, field: &str) -> usize {
    let mut index = index + field.len() + 1;

    if detail[index..].starts_with('"') {
        index += '"'.len_utf8();
        while index < detail.len() {
            let ch = detail[index..]
                .chars()
                .next()
                .expect("index is within a char boundary");
            index += ch.len_utf8();
            if ch == '"' {
                break;
            }
        }
        return index;
    }

    while index < detail.len() {
        let ch = detail[index..]
            .chars()
            .next()
            .expect("index is within a char boundary");
        if ch.is_whitespace() {
            break;
        }
        index += ch.len_utf8();
    }

    index
}

fn truncate_execution_detail(detail: &str, max_chars: usize) -> String {
    let mut value: String = detail.chars().take(max_chars).collect();
    if detail.chars().count() > max_chars {
        value.push_str("...");
    }
    value
}

// ---------------------------------------------------------------------------
// Envelope helpers
// ---------------------------------------------------------------------------

fn extract_sak_key(
    envelope: &CommandEnvelope<ApplicationCommand>,
) -> Result<SakKey, anyhow::Error> {
    match &envelope.payload {
        ApplicationCommand::OpprettSak(cmd) => Ok(SakKey::ClientReference(cmd.client_reference)),
        ApplicationCommand::OpprettInngaaendeJournalpost(cmd) => Ok(cmd.felles().sak_key.clone()),
        ApplicationCommand::OpprettUtgaaendeJournalpost(cmd) => Ok(cmd.felles().sak_key.clone()),
        ApplicationCommand::OpprettInterntNotatJournalpost(cmd) => Ok(cmd.felles().sak_key.clone()),
        ApplicationCommand::AvsluttSak(cmd) => Ok(cmd.sak_key.clone()),
        ApplicationCommand::SettSaksansvarlig(cmd) => Ok(cmd.sak_key.clone()),
    }
}

fn extract_sak_client_reference(envelope: &CommandEnvelope<ApplicationCommand>) -> Option<Uuid> {
    match &envelope.payload {
        ApplicationCommand::OpprettSak(cmd) => Some(cmd.client_reference),
        _ => None,
    }
}

fn extract_journalpost_client_reference(
    envelope: &CommandEnvelope<ApplicationCommand>,
) -> Option<Uuid> {
    match &envelope.payload {
        ApplicationCommand::OpprettInngaaendeJournalpost(cmd) => {
            Some(cmd.felles().client_reference)
        }
        ApplicationCommand::OpprettUtgaaendeJournalpost(cmd) => Some(cmd.felles().client_reference),
        ApplicationCommand::OpprettInterntNotatJournalpost(cmd) => {
            Some(cmd.felles().client_reference)
        }
        _ => None,
    }
}

fn extract_dokument_client_references(envelope: &CommandEnvelope<ApplicationCommand>) -> Vec<Uuid> {
    match &envelope.payload {
        ApplicationCommand::OpprettInngaaendeJournalpost(cmd) => cmd
            .felles()
            .dokumenter
            .iter()
            .map(|d| d.client_reference)
            .collect(),
        ApplicationCommand::OpprettUtgaaendeJournalpost(cmd) => cmd
            .felles()
            .dokumenter
            .iter()
            .map(|d| d.client_reference)
            .collect(),
        ApplicationCommand::OpprettInterntNotatJournalpost(cmd) => cmd
            .felles()
            .dokumenter
            .iter()
            .map(|d| d.client_reference)
            .collect(),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::{map_arkiv_feil_detail, safe_execution_detail};
    use domain::eksekvering::typer::EksekveringFeiltype;

    #[test]
    fn safe_execution_detail_strips_html2pdf_auth_body() {
        let detail = "html2pdf_auth_failed status=403 endpoint_host=renderer.example audience=renderer.example body=forbidden";

        let safe = safe_execution_detail(detail);

        assert_eq!(
            safe,
            "html2pdf_auth_failed status=403 endpoint_host=renderer.example audience=renderer.example"
        );
        assert!(!safe.contains("body="));
        assert!(!safe.contains("forbidden"));
    }

    #[test]
    fn safe_execution_detail_strips_html2pdf_client_error_body_from_error_chain() {
        let detail =
            "render failed: html2pdf_client_error status=404 endpoint_path=/render body=not_found";

        let safe = safe_execution_detail(detail);

        assert_eq!(
            safe,
            "html2pdf_client_error status=404 endpoint_path=/render"
        );
        assert!(!safe.contains("body="));
        assert!(!safe.contains("not_found"));
    }

    #[test]
    fn safe_execution_detail_preserves_html2pdf_server_error() {
        let detail = "html2pdf_server_error status=503 status_class=server_error endpoint_host=renderer.example external_error_message=\"renderer queue unavailable\"";

        assert_eq!(safe_execution_detail(detail), detail);
    }

    #[test]
    fn safe_execution_detail_preserves_html2pdf_request_failed() {
        let detail = "html2pdf_request_failed category=timeout endpoint_host=renderer.example";

        assert_eq!(safe_execution_detail(detail), detail);
    }

    #[test]
    fn safe_execution_detail_preserves_html2pdf_response_read_failed() {
        let detail =
            "html2pdf_response_read_failed category=body_read endpoint_host=renderer.example";

        assert_eq!(safe_execution_detail(detail), detail);
    }

    #[test]
    fn safe_execution_detail_redacts_sensitive_renderer_tokens() {
        let detail = "html2pdf_server_error token=secret Authorization: Bearer abc123 external_error_message=\"renderer failed\"";

        let safe = safe_execution_detail(detail);

        assert!(safe.contains("html2pdf_server_error"));
        assert!(safe.contains("external_error_message=\"renderer failed\""));
        assert!(!safe.contains("secret"));
        assert!(!safe.contains("abc123"));
        assert!(!safe.contains("body="));
        assert!(safe.contains("token=redacted"));
        assert!(safe.contains("Authorization:redacted"));
    }

    #[test]
    fn safe_execution_detail_preserves_external_response_message_and_redacts_secrets() {
        let detail = "html2pdf_client_error category=client_error status=400 external_error_message=\"invalid css token=secret Authorization: Bearer abc123\" endpoint_host=renderer.example";

        let safe = safe_execution_detail(detail);

        assert!(safe.starts_with("html2pdf_client_error"));
        assert!(safe.contains("external_error_message=\"invalid css token=redacted"));
        assert!(safe.contains("Authorization:redacted"));
        assert!(!safe.contains("secret"));
        assert!(!safe.contains("abc123"));
    }

    #[test]
    fn safe_execution_detail_strips_embedded_payload_for_renderer_detail() {
        let detail = "html2pdf_client_error status=400 {\"html\":\"<body>payload</body>\"}";

        let safe = safe_execution_detail(detail);

        assert_eq!(safe, "html2pdf_client_error status=400 [payload stripped]");
    }

    #[test]
    fn safe_execution_detail_bounds_renderer_detail() {
        let detail = format!(
            "html2pdf_server_error category=server_error {}",
            "a".repeat(800)
        );

        let safe = safe_execution_detail(&detail);

        assert!(safe.starts_with("html2pdf_server_error"));
        assert!(safe.len() <= 503);
        assert!(safe.ends_with("..."));
    }

    #[test]
    fn safe_execution_detail_strips_quoted_html2pdf_body_fields() {
        let detail = r#"html2pdf_client_error category=client_error status=400 body="<html>renderer failed</html>" error_body="free text with spaces" endpoint_host=renderer.example content_length=123"#;

        let safe = safe_execution_detail(detail);

        assert_eq!(
            safe,
            "html2pdf_client_error category=client_error status=400 endpoint_host=renderer.example content_length=123"
        );
        assert!(!safe.contains("body="));
        assert!(!safe.contains("error_body="));
        assert!(!safe.contains("renderer failed"));
        assert!(!safe.contains("free text"));
    }

    #[test]
    fn safe_execution_detail_preserves_render_diagnostics() {
        let detail = "render_html_mal_lager_unavailable mal_referanse=019e3d15-0000-7000-8000-000000000001 error=\"object store unavailable\"";

        assert_eq!(safe_execution_detail(detail), detail);
    }

    #[test]
    fn safe_execution_detail_redacts_render_diagnostics() {
        let detail = "rendered_dokument_save_failed token=secret Authorization: Bearer abc123 Authorization: Basic xyz789 credential=abc x-api-key=def Bearer=ghi Basic=jkl";

        let safe = safe_execution_detail(detail);

        assert!(safe.starts_with("rendered_dokument_save_failed"));
        assert!(!safe.contains("secret"));
        assert!(!safe.contains("abc123"));
        assert!(!safe.contains("xyz789"));
        assert!(!safe.contains("credential=abc"));
        assert!(!safe.contains("x-api-key=def"));
        assert!(!safe.contains("Bearer=ghi"));
        assert!(!safe.contains("Basic=jkl"));
        assert!(safe.contains("credential=redacted"));
        assert!(safe.contains("x-api-key=redacted"));
        assert!(safe.contains("Bearer=redacted"));
        assert!(safe.contains("Basic=redacted"));
    }

    #[test]
    fn safe_execution_detail_bounds_render_diagnostics() {
        let detail = format!("render_state_update_failed error={}", "a".repeat(800));

        let safe = safe_execution_detail(&detail);

        assert!(safe.starts_with("render_state_update_failed"));
        assert!(safe.len() <= 503);
        assert!(safe.ends_with("..."));
    }

    #[test]
    fn safe_execution_detail_preserves_sikri_code() {
        let detail =
            "Sikri error sikri_missing_document_content sikri_recoverability=irrecoverable";

        assert_eq!(
            safe_execution_detail(detail),
            "sikri_missing_document_content"
        );
    }

    #[test]
    fn arkivmapping_contract_failures_are_irrecoverable() {
        for detail in [
            "arkivmapping_dokument_fact_mangler dokument_id=123 sikri_recoverability=irrecoverable",
            "arkivmapping_rendered_dokument_mangler dokument_id=456 sikri_recoverability=irrecoverable",
            "arkivmapping_dokumentform_mismatch dokument_id=789 sikri_recoverability=irrecoverable",
        ] {
            let feil = map_arkiv_feil_detail(detail, safe_execution_detail(detail));

            assert_eq!(feil.feiltype, EksekveringFeiltype::Irrecoverable);
            assert!(feil.melding.starts_with("arkivmapping_"));
            assert!(!feil.melding.contains("sikri_recoverability"));
        }
    }

    #[test]
    fn safe_execution_detail_preserves_arkivmapping_category_after_recoverability_marker() {
        let detail =
            "sikri_recoverability=irrecoverable arkivmapping_dokumentform_mismatch dokument_id=789";

        let feil = map_arkiv_feil_detail(detail, safe_execution_detail(detail));

        assert_eq!(feil.feiltype, EksekveringFeiltype::Irrecoverable);
        assert!(
            feil.melding
                .starts_with("arkivmapping_dokumentform_mismatch")
        );
        assert!(!feil.melding.contains("sikri_recoverability"));
    }

    #[test]
    fn safe_execution_detail_keeps_sikri_upstream_fallback() {
        let detail = "upstream failed sikri_recoverability=recoverable";

        assert_eq!(safe_execution_detail(detail), "execution_upstream_error");
    }

    #[test]
    fn safe_execution_detail_collapses_generic_errors() {
        assert_eq!(
            safe_execution_detail("database unavailable"),
            "execution_error"
        );
    }
}
