mod dokument_handlers;
mod journalpost_handlers;
mod lifecycle_publisher;
mod sak_handlers;

use anyhow::Context;
use domain::eksekvering::id::{SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::{
    planlegg_neste_handling, BlockedReason, CommandStateDecision, DomainViolation, SakMedBarn,
};
use domain::eksekvering::typer::{command_metadata, CommandTypeCode, EksekveringFeil};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::query::queries::SakKey;
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
use crate::command::services::execution_registration::command_target_for_type;

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
        envelope: CommandEnvelope<Command>,
        attempt: u32,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let (command_type, _) = command_metadata(&envelope.payload);
        let sak_id = self.resolve_sak_id_for_envelope(&envelope).await?;
        let journalpost_id = self
            .resolve_journalpost_id_for_envelope(&envelope, command_type)
            .await?;
        let target = command_target_for_type(command_type, journalpost_id)?;

        let sak_med_barn = self.hent_sak_med_barn(sak_id).await?;
        match planlegg_neste_handling(command_type, target, &sak_med_barn) {
            CommandStateDecision::Ready(operasjon) => {
                match self
                    .utfoer_operasjon(&envelope, &sak_med_barn, operasjon)
                    .await
                {
                    Ok(()) => {
                        let _ = self.wakeup_after_operation(sak_id, operasjon).await;
                        let oppdatert_sak_med_barn = self.hent_sak_med_barn(sak_id).await?;
                        let neste_beslutning =
                            planlegg_neste_handling(command_type, target, &oppdatert_sak_med_barn);
                        self.materialiser_beslutning(&envelope, attempt, neste_beslutning)
                            .await
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

    async fn hent_sak_med_barn(&self, sak_id: SkuffenSakId) -> Result<SakMedBarn, anyhow::Error> {
        self.entity_tilstand_repo
            .hent_sak_med_barn(sak_id)
            .await
            .context("Feil ved henting av sak med barn")?
            .ok_or_else(|| anyhow::anyhow!("Sak {} finnes ikke i tilstandstabeller", sak_id.0))
    }

    async fn materialiser_beslutning(
        &self,
        envelope: &CommandEnvelope<Command>,
        attempt: u32,
        beslutning: CommandStateDecision,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        match beslutning {
            CommandStateDecision::Ready(_) => Ok(ExecutionOutcome::Klar),
            CommandStateDecision::Blocked(reason) => {
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
        envelope: &CommandEnvelope<Command>,
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
        envelope: &CommandEnvelope<Command>,
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
        envelope: &CommandEnvelope<Command>,
        command_type: CommandTypeCode,
    ) -> Result<Option<SkuffenJournalpostId>, anyhow::Error> {
        let Some(client_reference) = extract_journalpost_client_reference(envelope) else {
            return Ok(None);
        };

        match command_type {
            CommandTypeCode::OpprettInngaaendeJournalpost
            | CommandTypeCode::OpprettUtgaaendeJournalpost
            | CommandTypeCode::OpprettInterntNotatJournalpost => self
                .id_mapping
                .hent_journalpost_id_fra_mapping(client_reference)
                .await
                .context("Feil ved oppslag av journalpost_id fra client_reference")?
                .map(Some)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Fant ikke skuffen_id for journalpost client_reference {}",
                        client_reference
                    )
                }),
            CommandTypeCode::OpprettSak
            | CommandTypeCode::AvsluttSak
            | CommandTypeCode::SettSaksansvarlig => Ok(None),
        }
    }

    fn map_arkiv_feil(&self, err: anyhow::Error) -> EksekveringFeil {
        let original = err.to_string();
        let message = safe_execution_detail(&original);

        if original.contains("sikri_recoverability=irrecoverable") {
            return EksekveringFeil::irrecoverable(message);
        }

        EksekveringFeil::recoverable(message)
    }
}

fn safe_execution_detail(detail: &str) -> String {
    let stripped = detail
        .replace("sikri_recoverability=irrecoverable", "")
        .replace("sikri_recoverability=recoverable", "");
    let normalized = stripped.split_whitespace().collect::<Vec<_>>().join(" ");

    if let Some(code) = normalized
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .find(|token| token.starts_with("sikri_"))
    {
        return code.to_string();
    }

    if detail.contains("sikri_recoverability=") {
        return "execution_upstream_error".to_string();
    }

    "execution_error".to_string()
}

fn blocked_detail(reason: BlockedReason) -> String {
    format!(
        "{} trigger_category={}",
        reason.safe_detail(),
        reason.trigger_category().as_code()
    )
}

fn invalid_detail(violation: DomainViolation) -> String {
    violation.safe_detail().to_string()
}

// ---------------------------------------------------------------------------
// Envelope helpers
// ---------------------------------------------------------------------------

fn extract_sak_key(envelope: &CommandEnvelope<Command>) -> Result<SakKey, anyhow::Error> {
    match &envelope.payload {
        Command::OpprettSak(cmd) => Ok(SakKey::ClientReference(cmd.client_reference)),
        Command::OpprettInngåendeJournalpost(cmd) => Ok(cmd.felles.sak_key.clone()),
        Command::OpprettUtgåendeJournalpost(cmd) => Ok(cmd.felles.sak_key.clone()),
        Command::OpprettInterntNotatJournalpost(cmd) => Ok(cmd.felles.sak_key.clone()),
        Command::AvsluttSak(cmd) => Ok(cmd.sak_key.clone()),
        Command::SettSaksansvarlig(cmd) => Ok(cmd.sak_key.clone()),
    }
}

fn extract_sak_client_reference(envelope: &CommandEnvelope<Command>) -> Option<Uuid> {
    match &envelope.payload {
        Command::OpprettSak(cmd) => Some(cmd.client_reference),
        _ => None,
    }
}

fn extract_journalpost_client_reference(envelope: &CommandEnvelope<Command>) -> Option<Uuid> {
    match &envelope.payload {
        Command::OpprettInngåendeJournalpost(cmd) => Some(cmd.felles.client_reference),
        Command::OpprettUtgåendeJournalpost(cmd) => Some(cmd.felles.client_reference),
        Command::OpprettInterntNotatJournalpost(cmd) => Some(cmd.felles.client_reference),
        _ => None,
    }
}

fn extract_dokument_client_references(envelope: &CommandEnvelope<Command>) -> Vec<Uuid> {
    match &envelope.payload {
        Command::OpprettInngåendeJournalpost(cmd) => cmd
            .felles
            .dokumenter
            .iter()
            .map(|d| d.client_reference)
            .collect(),
        Command::OpprettUtgåendeJournalpost(cmd) => cmd
            .felles
            .dokumenter
            .iter()
            .map(|d| d.client_reference)
            .collect(),
        Command::OpprettInterntNotatJournalpost(cmd) => cmd
            .felles
            .dokumenter
            .iter()
            .map(|d| d.client_reference)
            .collect(),
        _ => vec![],
    }
}
