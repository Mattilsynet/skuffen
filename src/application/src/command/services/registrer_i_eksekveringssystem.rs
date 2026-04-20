use crate::command::services::execution_registration::resolve_registration;

use anyhow::Result;
use async_trait::async_trait;
use domain::eksekvering::execution::EksekveringStatus;
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::tilstand::JournalpostType;
use domain::eksekvering::tilstand::{
    er_ferdig, neste_handling, oensket_sluttilstand_for_journalpost, SakTilstand,
};
use domain::eksekvering::typer::{
    command_metadata, CommandLifecycleEvent, CommandTypeCode, EksekveringFeiltype,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

use crate::command::ports::command_execution_port::{
    CommandExecutionRepository, EksekveringsregistreringResultat, NyKommandoEksekvering,
};
use crate::command::ports::eksekvering_port::EksekveringStatusPublisher;
use crate::command::ports::entity_tilstand_port::EntityTilstandRepository;
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::registrer_i_eksekveringssystem_port::RegistrerIEksekveringssystemUseCase;
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::status::{utfores_error_event, utfores_venter_event};

/// Tynn registrering inn i eksekveringssystemet for kommandoer som allerede er validert.
///
/// Oppretter entity-tilstandsrader, evaluerer klarhet via `neste_handling`,
/// registrerer kommandoen i `command_execution`, og publiserer `utfores::venter`
/// når registreringen faktisk ble opprettet.
pub struct RegistrerIEksekveringssystemService {
    execution_repo: Box<dyn CommandExecutionRepository>,
    entity_tilstand_repo: Box<dyn EntityTilstandRepository>,
    id_mapping_repo: Box<dyn IdMappingRepository>,
    status_publisher: Box<dyn EksekveringStatusPublisher>,
    outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
}

impl RegistrerIEksekveringssystemService {
    pub fn new(
        execution_repo: Box<dyn CommandExecutionRepository>,
        entity_tilstand_repo: Box<dyn EntityTilstandRepository>,
        id_mapping_repo: Box<dyn IdMappingRepository>,
        status_publisher: Box<dyn EksekveringStatusPublisher>,
        outward_status_projector: Box<dyn CommandOutwardStatusProjector>,
    ) -> Self {
        Self {
            execution_repo,
            entity_tilstand_repo,
            id_mapping_repo,
            status_publisher,
            outward_status_projector,
        }
    }

    async fn ensure_registrert_i_eksekveringssystem(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<(EksekveringsregistreringResultat, EksekveringStatus)> {
        let registration = resolve_registration(self.id_mapping_repo.as_ref(), envelope).await?;
        let command_type = command_metadata(&envelope.payload).0;

        self.opprett_entity_tilstander(envelope, &registration)
            .await?;

        let status = self
            .evaluer_klarhet(registration.sak_id(), command_type)
            .await?;

        let ny = NyKommandoEksekvering {
            envelope: envelope.clone(),
            command_type,
            sak_id: registration.sak_id(),
            journalpost_id: registration.journalpost_id(),
            status,
            last_detail: if status == EksekveringStatus::Feil {
                Some("Tilstandsfeil ved registrering".to_string())
            } else {
                None
            },
        };

        let resultat = self.execution_repo.opprett(ny).await?;
        Ok((resultat, status))
    }

    async fn opprett_entity_tilstander(
        &self,
        envelope: &CommandEnvelope<Command>,
        registration: &super::execution_registration::ResolvedRegistration,
    ) -> Result<()> {
        match &envelope.payload {
            Command::OpprettSak(_) => {
                let sak_id = registration
                    .sak_id()
                    .ok_or_else(|| anyhow::anyhow!("Mangler sak_id for OpprettSak"))?;
                self.entity_tilstand_repo
                    .opprett_sak_tilstand(sak_id, SakTilstand::Opprettet, envelope.command_id)
                    .await?;
            }
            Command::OpprettInngåendeJournalpost(_) => {
                self.opprett_journalpost_tilstander(
                    envelope,
                    registration,
                    JournalpostType::Inngaende,
                )
                .await?;
            }
            Command::OpprettUtgåendeJournalpost(_) => {
                self.opprett_journalpost_tilstander(
                    envelope,
                    registration,
                    JournalpostType::Utgaaende,
                )
                .await?;
            }
            Command::OpprettInterntNotatJournalpost(_) => {
                self.opprett_journalpost_tilstander(
                    envelope,
                    registration,
                    JournalpostType::InterntNotat,
                )
                .await?;
            }
            Command::AvsluttSak(_) => {
                let sak_id = registration
                    .sak_id()
                    .ok_or_else(|| anyhow::anyhow!("Mangler sak_id for AvsluttSak"))?;
                self.entity_tilstand_repo
                    .oppdater_sak_oensket_tilstand(sak_id, SakTilstand::Avsluttet)
                    .await?;
            }
        }
        Ok(())
    }

    async fn opprett_journalpost_tilstander(
        &self,
        envelope: &CommandEnvelope<Command>,
        registration: &super::execution_registration::ResolvedRegistration,
        journalposttype: JournalpostType,
    ) -> Result<()> {
        let sak_id = registration
            .sak_id()
            .ok_or_else(|| anyhow::anyhow!("Mangler sak_id for journalpost-kommando"))?;
        let jp_id = registration
            .journalpost_id()
            .ok_or_else(|| anyhow::anyhow!("Mangler journalpost_id for journalpost-kommando"))?;

        self.entity_tilstand_repo
            .opprett_journalpost_tilstand(
                jp_id,
                sak_id,
                journalposttype,
                false,
                oensket_sluttilstand_for_journalpost(journalposttype),
                envelope.command_id,
            )
            .await?;

        for dok in &registration.dokumenter {
            self.entity_tilstand_repo
                .opprett_dokument_tilstand(dok.dokument_id, jp_id, envelope.command_id)
                .await?;
        }

        Ok(())
    }

    async fn evaluer_klarhet(
        &self,
        sak_id: Option<SkuffenSakId>,
        command_type: CommandTypeCode,
    ) -> Result<EksekveringStatus> {
        let Some(sak_id) = sak_id else {
            return Err(anyhow::anyhow!("Mangler sak_id"));
        };

        // OpprettSak: always Klar (we just created tilstand row as IkkeRealisert → Opprettet)
        if command_type == CommandTypeCode::OpprettSak {
            return Ok(EksekveringStatus::Klar);
        }

        let Some(sak_med_barn) = self.entity_tilstand_repo.hent_sak_med_barn(sak_id).await? else {
            return Ok(EksekveringStatus::BlokkertVenter);
        };

        match neste_handling(command_type, &sak_med_barn) {
            Ok(Some(_)) => Ok(EksekveringStatus::Klar),
            Ok(None) => {
                if er_ferdig(&sak_med_barn) {
                    Ok(EksekveringStatus::Ok)
                } else {
                    Ok(EksekveringStatus::BlokkertVenter)
                }
            }
            Err(feil) => match feil.feiltype {
                EksekveringFeiltype::Blocked => Ok(EksekveringStatus::BlokkertVenter),
                _ => Ok(EksekveringStatus::Feil),
            },
        }
    }

    async fn emit_status(&self, event: CommandLifecycleEvent) -> Result<()> {
        self.status_publisher.publiser_status(event).await
    }
}

#[async_trait]
impl RegistrerIEksekveringssystemUseCase for RegistrerIEksekveringssystemService {
    async fn handle(&self, envelope: &CommandEnvelope<Command>) -> Result<()> {
        let (registrering, status) = self
            .ensure_registrert_i_eksekveringssystem(envelope)
            .await?;

        let context = self
            .outward_status_projector
            .resolve_context(envelope)
            .await?;

        match status {
            EksekveringStatus::Feil => {
                if matches!(registrering, EksekveringsregistreringResultat::Nyregistrert) {
                    self.emit_status(utfores_error_event(
                        envelope,
                        "Tilstandsfeil ved registrering",
                        None,
                        context,
                        Some(1),
                    ))
                    .await?;
                }
            }
            EksekveringStatus::Klar | EksekveringStatus::BlokkertVenter | EksekveringStatus::Ok
                if registrering.skal_publisere_utfores_venter() =>
            {
                self.emit_status(utfores_venter_event(envelope, context, Some(1)))
                    .await?;
                self.execution_repo
                    .marker_utfores_venter_publisert(envelope.command_id)
                    .await?;
            }
            _ => {}
        }

        Ok(())
    }
}
