mod dokument_handlers;
mod journalpost_handlers;
mod lifecycle_publisher;
mod sak_handlers;

use anyhow::Context;
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::tilstand::{er_ferdig, neste_handling, SakMedBarn};
use domain::eksekvering::typer::{command_metadata, EksekveringFeil};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::query::queries::SakKey;
use uuid::Uuid;

use crate::command::ports::eksekvering_port::{
    ArkivGateway, EksekveringKvitteringPublisher, EksekveringStatusPublisher,
};
use crate::command::ports::entity_tilstand_port::EntityTilstandRepository;
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::ports::ventende_kommando_wakeup_port::VentendeKommandoWakeup;

pub struct EksekverKommandoService {
    entity_tilstand_repo: Box<dyn EntityTilstandRepository>,
    arkiv_gateway: Box<dyn ArkivGateway>,
    id_mapping: Box<dyn IdMappingRepository>,
    status_publisher: Box<dyn EksekveringStatusPublisher>,
    done_publisher: Box<dyn EksekveringKvitteringPublisher>,
    outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
    wakeup_service: Box<dyn VentendeKommandoWakeup>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionOutcome {
    Ok,
    BlokkertVenter { last_error: Option<String> },
    Retrying { last_error: Option<String> },
    Feil { last_error: Option<String> },
}

impl EksekverKommandoService {
    pub fn new(
        entity_tilstand_repo: Box<dyn EntityTilstandRepository>,
        arkiv_gateway: Box<dyn ArkivGateway>,
        status_publisher: Box<dyn EksekveringStatusPublisher>,
        done_publisher: Box<dyn EksekveringKvitteringPublisher>,
        id_mapping: Box<dyn IdMappingRepository>,
        outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
        wakeup_service: Box<dyn VentendeKommandoWakeup>,
    ) -> Self {
        Self {
            entity_tilstand_repo,
            arkiv_gateway,
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

        loop {
            let sak_med_barn = self
                .entity_tilstand_repo
                .hent_sak_med_barn(sak_id)
                .await
                .context("Feil ved henting av sak med barn")?
                .ok_or_else(|| {
                    anyhow::anyhow!("Sak {} finnes ikke i tilstandstabeller", sak_id.0)
                })?;

            match neste_handling(command_type, &sak_med_barn) {
                Ok(Some(operasjon)) => {
                    match self
                        .utfoer_operasjon(&envelope, &sak_med_barn, operasjon)
                        .await
                    {
                        Ok(()) => {
                            // Loop continues — reload and check again
                        }
                        Err(feil) => {
                            let _ = self.wakeup_service.etter_sak_endret(sak_id).await;
                            return self.map_feil_til_outcome(&envelope, feil, attempt).await;
                        }
                    }
                }
                Ok(None) => {
                    let _ = self.wakeup_service.etter_sak_endret(sak_id).await;
                    if er_ferdig(&sak_med_barn) {
                        return self.publish_success(&envelope, attempt).await;
                    } else {
                        return self.publish_blocked(&envelope, attempt).await;
                    }
                }
                Err(feil) => {
                    let _ = self.wakeup_service.etter_sak_endret(sak_id).await;
                    return self.map_feil_til_outcome(&envelope, feil, attempt).await;
                }
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
            ArkivOperasjon::Journalfoer { journalpost_id } => {
                self.journalfoer(envelope, sak, journalpost_id).await
            }
            ArkivOperasjon::Avskriv { journalpost_id } => {
                self.avskriv(envelope, sak, journalpost_id).await
            }
            ArkivOperasjon::AvsluttSak { sak_id: _ } => self.avslutt_sak(envelope, sak).await,
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

    fn map_arkiv_feil(&self, err: anyhow::Error) -> EksekveringFeil {
        let original = err.to_string();
        let message = original
            .replace("sikri_recoverability=irrecoverable", "")
            .replace("sikri_recoverability=recoverable", "")
            .trim()
            .to_string();

        if original.contains("sikri_recoverability=irrecoverable") {
            return EksekveringFeil::irrecoverable(message);
        }

        EksekveringFeil::recoverable(message)
    }
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
