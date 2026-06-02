use crate::command::services::command_state_decision::registration_initial_status;
use crate::command::services::execution_registration::{
    domain_command_for_type, resolve_registration, SakResolutionOrigin,
};

use anyhow::Result;
use async_trait::async_trait;
use domain::eksekvering::execution::EksekveringStatus;
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::tilstand::JournalpostType;
use domain::eksekvering::tilstand::{
    planlegg_neste_handling, BlockedReason, CommandStateDecision, DokumentTilstand,
};
use domain::eksekvering::typer::{command_metadata, CommandLifecycleEvent, CommandTypeCode};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::dokument::Dokumentform;

use crate::command::ports::command_execution_port::{
    CommandExecutionRepository, EksekveringsregistreringResultat, NyKommandoEksekvering,
};
use crate::command::ports::eksekvering_port::EksekveringStatusPublisher;
use crate::command::ports::entity_tilstand_port::EntityTilstandRepository;
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::registrer_i_eksekveringssystem_port::RegistrerIEksekveringssystemUseCase;
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::status::utfores_venter_event;

/// Tynn registrering inn i eksekveringssystemet for kommandoer som allerede er validert.
///
/// Oppretter entity-tilstandsrader, setter initial køstatus via `planlegg_neste_handling` / `CommandStateDecision`,
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

        // Seed sak_tilstand for validated ArkivId before any command-specific writes.
        // This ensures archive-validated cases have local Opprettet state with saksnummer.
        self.seed_arkiv_id_provenance(envelope.command_id, &registration)
            .await?;

        self.opprett_entity_tilstander(envelope, &registration)
            .await?;

        let (status, last_detail) = self
            .sett_initiell_eksekveringstatus(
                registration.sak_id(),
                registration.journalpost_id(),
                command_type,
            )
            .await?;

        let ny = NyKommandoEksekvering {
            envelope: envelope.clone(),
            command_type,
            sak_id: registration.sak_id(),
            journalpost_id: registration.journalpost_id(),
            status,
            last_detail,
        };

        let resultat = self.execution_repo.opprett(ny).await?;
        Ok((resultat, status))
    }

    /// Seeding orchestration: ensure sak_tilstand exists for ArkivId-validated cases.
    /// Idempotent: no overwrite if row already exists.
    /// Only seeds for SakResolutionOrigin::ArkivId; skips ClientReference and OpprettSak.
    async fn seed_arkiv_id_provenance(
        &self,
        command_id: uuid::Uuid,
        registration: &super::execution_registration::ResolvedRegistration,
    ) -> Result<()> {
        if let Some(sak_reg) = registration.sak.as_ref() {
            if let SakResolutionOrigin::ArkivId { saksnummer } = &sak_reg.origin {
                self.entity_tilstand_repo
                    .ensure_sak_tilstand_for_arkiv_id(sak_reg.sak_id, saksnummer, command_id)
                    .await?;
            }
        }
        Ok(())
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
                    .opprett_sak_tilstand(sak_id, envelope.command_id)
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
            Command::AvsluttSak(_) => {}
            Command::SettSaksansvarlig(cmd) => {
                let sak_id = registration
                    .sak_id()
                    .ok_or_else(|| anyhow::anyhow!("Mangler sak_id for SettSaksansvarlig"))?;
                self.entity_tilstand_repo
                    .oppdater_oensket_saksansvarlig(
                        sak_id,
                        &cmd.saksbehandler_id,
                        &cmd.saksbehandler_enhet,
                    )
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
                envelope.command_id,
            )
            .await?;

        let dokumenter = dokumenter_for_envelope(envelope);
        for (index, dok) in registration.dokumenter.iter().enumerate() {
            let schema_dokument = dokumenter.get(index).ok_or_else(|| {
                anyhow::anyhow!("Mangler dokumentform for dokument {}", dok.dokument_id.0)
            })?;
            let (tilstand, mal_referanse, felter) = match &schema_dokument.form {
                Dokumentform::Bytes { .. } => (DokumentTilstand::IkkeRealisert, None, Vec::new()),
                Dokumentform::HtmlTemplate {
                    mal_referanse,
                    felter,
                } => (
                    DokumentTilstand::AvventerRendring,
                    Some(*mal_referanse),
                    felter.clone(),
                ),
            };
            self.entity_tilstand_repo
                .opprett_dokument_tilstand(
                    dok.dokument_id,
                    jp_id,
                    tilstand,
                    mal_referanse,
                    felter,
                    envelope.command_id,
                )
                .await?;
        }

        Ok(())
    }

    async fn sett_initiell_eksekveringstatus(
        &self,
        sak_id: Option<SkuffenSakId>,
        journalpost_id: Option<domain::eksekvering::id::SkuffenJournalpostId>,
        command_type: CommandTypeCode,
    ) -> Result<(EksekveringStatus, Option<String>)> {
        let Some(sak_id) = sak_id else {
            return Err(anyhow::anyhow!("Mangler sak_id"));
        };

        // OpprettSak: always Klar (we just created tilstand row as IkkeRealisert → Opprettet)
        if command_type == CommandTypeCode::OpprettSak {
            return Ok((EksekveringStatus::Klar, None));
        }

        let Some(sak_med_barn) = self.entity_tilstand_repo.hent_sak_med_barn(sak_id).await? else {
            return Ok(registration_initial_status(CommandStateDecision::Blocked(
                BlockedReason::EntityMissing,
            )));
        };

        let domain_command = domain_command_for_type(command_type, sak_id, journalpost_id)?;

        Ok(registration_initial_status(planlegg_neste_handling(
            &domain_command,
            &sak_med_barn,
        )))
    }

    async fn emit_status(&self, event: CommandLifecycleEvent) -> Result<()> {
        self.status_publisher.publiser_status(event).await
    }
}

fn dokumenter_for_envelope(
    envelope: &CommandEnvelope<Command>,
) -> &[lib_schemas::skuffen::dokument::Dokument] {
    match &envelope.payload {
        Command::OpprettInngåendeJournalpost(cmd) => &cmd.felles.dokumenter,
        Command::OpprettUtgåendeJournalpost(cmd) => &cmd.felles.dokumenter,
        Command::OpprettInterntNotatJournalpost(cmd) => &cmd.felles.dokumenter,
        _ => &[],
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
            EksekveringStatus::Klar | EksekveringStatus::BlokkertVenter
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
