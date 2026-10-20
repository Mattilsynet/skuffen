use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Context;
use domain::eksekvering::gjenopptak::{Gjenopptak, vurder_command};
use domain::eksekvering::id::SkuffenSakId;
use uuid::Uuid;

use crate::admin::model::AdminSakNokkel;
use crate::admin::ports::admin_retry_port::{
    AdminRetryRepository, FrigivelseFeil, OriginalCommandKilde, RetryGrunnlag,
};
use crate::command::ports::{command_port::CommandRepository, entitet_port::EntitetRepository};
use crate::command::services::ingest_command::finn_sak_id;
use crate::command::services::validate_command::ValidateCommandService;
use crate::command::{Command, CommandEnvelope};

/// Øvre grense for antall kommandoer behandlet per forespørsel.
pub const MAKS_COMMANDS_PER_RETRY: usize = 100;

/// Avvisning i preflight eller teknisk feil under behandling.
#[derive(Debug, thiserror::Error)]
pub enum AdminRetryError {
    #[error("ingen kommandoer oppgitt")]
    TomForesporsel,
    #[error("for mange kommandoer i én forespørsel")]
    ForMange,
    #[error("command not found")]
    CommandNotFound(Vec<Uuid>),
    #[error("command is discarded")]
    Forkastet(Vec<Uuid>),
    #[error("sak not found")]
    SakNotFound,
    #[error("original command not available")]
    OriginalIkkeTilgjengelig(Vec<Uuid>),
    #[error("sak har operasjoner med uavklart utfall")]
    SakHarUavklartUtfall(Vec<SkuffenSakId>),
    #[error("saken er opptatt")]
    Opptatt,
    #[error("admin retry failed")]
    Teknisk(#[from] anyhow::Error),
}

impl From<FrigivelseFeil> for AdminRetryError {
    fn from(feil: FrigivelseFeil) -> Self {
        match feil {
            FrigivelseFeil::SakHarUavklartUtfall(saker) => Self::SakHarUavklartUtfall(saker),
            FrigivelseFeil::Opptatt => Self::Opptatt,
            FrigivelseFeil::Repository(err) => Self::Teknisk(err),
        }
    }
}

/// Slipper stoppede kommandoer inn i ordinær behandling igjen.
///
/// Ingen egne arkivkall og ingen egen utføring: udekomponerte kommandoer går
/// gjennom validatoren med originalen, og eksisterende operasjoner frigis til
/// executoren. Uavklart utfall på en berørt sak avviser hele forespørselen
/// før frigivelse og validering. Planlegging kan lagre manglende sakskoblinger.
pub struct AdminRetryService {
    repository: Arc<dyn AdminRetryRepository>,
    originaler: Arc<dyn OriginalCommandKilde>,
    command_repo: Arc<dyn CommandRepository>,
    entitet_repo: Arc<dyn EntitetRepository>,
    validator: ValidateCommandService,
}

struct Planlagt {
    grunnlag: RetryGrunnlag,
    handling: Handling,
}

enum Handling {
    Revalider(Box<CommandEnvelope<Command>>),
    FrigiOperasjoner,
    IngenEndring,
}

impl AdminRetryService {
    pub fn new(
        repository: Arc<dyn AdminRetryRepository>,
        originaler: Arc<dyn OriginalCommandKilde>,
        command_repo: Arc<dyn CommandRepository>,
        entitet_repo: Arc<dyn EntitetRepository>,
        validator: ValidateCommandService,
    ) -> Self {
        Self {
            repository,
            originaler,
            command_repo,
            entitet_repo,
            validator,
        }
    }

    #[tracing::instrument(skip_all, name = "admin.retry.sak", fields(selection = "sak"))]
    pub async fn handle_sak(&self, key: AdminSakNokkel) -> Result<(), AdminRetryError> {
        let command_ids = self
            .repository
            .hent_commands_for_sak(key, MAKS_COMMANDS_PER_RETRY + 1)
            .await?
            .ok_or(AdminRetryError::SakNotFound)?;

        if command_ids.len() > MAKS_COMMANDS_PER_RETRY {
            return Err(AdminRetryError::ForMange);
        }

        tracing::info!(
            command_ids = ?command_ids,
            antall_commands = command_ids.len(),
            "admin retry: sak valgt med {} kommandoer",
            command_ids.len()
        );

        if command_ids.is_empty() {
            return Ok(());
        }

        self.handle(command_ids).await
    }

    #[tracing::instrument(
        skip_all,
        name = "admin.retry",
        fields(antall_commands = command_ids.len())
    )]
    pub async fn handle(&self, command_ids: Vec<Uuid>) -> Result<(), AdminRetryError> {
        let command_ids = unike_i_rekkefolge(command_ids);
        if command_ids.is_empty() {
            return Err(AdminRetryError::TomForesporsel);
        }
        if command_ids.len() > MAKS_COMMANDS_PER_RETRY {
            return Err(AdminRetryError::ForMange);
        }

        let planlagt = self.planlegg(&command_ids).await?;

        let saker: Vec<SkuffenSakId> = unike_i_rekkefolge(
            planlagt
                .iter()
                .filter_map(|plan| plan.grunnlag.sak_id)
                .collect(),
        );
        let frigis: Vec<Uuid> = planlagt
            .iter()
            .filter(|plan| matches!(plan.handling, Handling::FrigiOperasjoner))
            .map(|plan| plan.grunnlag.command_id)
            .collect();

        for plan in &planlagt {
            tracing::info!(
                command_id = %plan.grunnlag.command_id,
                command_type = plan.grunnlag.command_type.as_code(),
                sak_id = plan.grunnlag.sak_id.map(|id| id.0.to_string()),
                "admin retry: forsøker {}",
                plan.grunnlag.command_type.as_code()
            );
        }

        // Preflight stopper frigivelse og validering, men sakskoblinger kan være lagret.
        self.repository.frigi(&saker, &frigis).await?;

        for plan in planlagt {
            if let Handling::Revalider(original) = plan.handling {
                self.validator.handle(*original).await.with_context(|| {
                    format!("admin retry: command_id={}", plan.grunnlag.command_id)
                })?;
            }
        }

        Ok(())
    }

    /// Henter grunnlaget og lagrer eventuelle manglende sakskoblinger før frigivelse.
    async fn planlegg(&self, command_ids: &[Uuid]) -> Result<Vec<Planlagt>, AdminRetryError> {
        let grunnlag = self.repository.hent_grunnlag(command_ids).await?;

        let ukjente: Vec<Uuid> = command_ids
            .iter()
            .filter(|id| !grunnlag.iter().any(|g| g.command_id == **id))
            .copied()
            .collect();
        if !ukjente.is_empty() {
            return Err(AdminRetryError::CommandNotFound(ukjente));
        }

        // Forkasting er endelig (SKU-0024); klienten sender en ny kommando.
        let forkastede: Vec<Uuid> = command_ids
            .iter()
            .filter(|id| grunnlag.iter().any(|g| g.command_id == **id && g.forkastet))
            .copied()
            .collect();
        if !forkastede.is_empty() {
            return Err(AdminRetryError::Forkastet(forkastede));
        }

        let mut grunnlag: HashMap<Uuid, RetryGrunnlag> =
            grunnlag.into_iter().map(|g| (g.command_id, g)).collect();
        let mut planlagt = Vec::with_capacity(command_ids.len());
        let mut mangler_original = Vec::new();
        for command_id in command_ids {
            let Some(mut grunnlag) = grunnlag.remove(command_id) else {
                continue;
            };
            let handling = match vurder_command(grunnlag.dekomponert, &grunnlag.operasjoner) {
                Gjenopptak::IngenEndring => Handling::IngenEndring,
                Gjenopptak::FrigiOperasjoner => Handling::FrigiOperasjoner,
                Gjenopptak::Revalider => {
                    let Some(original) = self
                        .originaler
                        .hent(grunnlag.command_id, grunnlag.command_type)
                        .await
                        .with_context(|| format!("hent original: command_id={command_id}"))?
                    else {
                        mangler_original.push(grunnlag.command_id);
                        continue;
                    };
                    if grunnlag.sak_id.is_none() {
                        grunnlag.sak_id = self.knytt_sak(&original).await?;
                    }
                    Handling::Revalider(Box::new(original))
                }
            };
            planlagt.push(Planlagt { grunnlag, handling });
        }

        if !mangler_original.is_empty() {
            return Err(AdminRetryError::OriginalIkkeTilgjengelig(mangler_original));
        }
        Ok(planlagt)
    }

    /// Kommandoer mottatt før koblingen fantes, får den her. Samme regel som
    /// ved mottak: eksisterende sak slås opp og koblingen lagres.
    async fn knytt_sak(
        &self,
        original: &CommandEnvelope<Command>,
    ) -> Result<Option<SkuffenSakId>, AdminRetryError> {
        let command_id = original.command_id;
        let sak_id = finn_sak_id(self.entitet_repo.as_ref(), &original.payload.sak_key())
            .await
            .with_context(|| format!("finn sak: command_id={command_id}"))?;
        if let Some(sak_id) = sak_id {
            self.command_repo
                .knytt_sak(original.command_id, sak_id)
                .await
                .with_context(|| format!("knytt sak: command_id={command_id}"))?;
        }
        Ok(sak_id)
    }
}

fn unike_i_rekkefolge<T: Copy + Eq + std::hash::Hash>(verdier: Vec<T>) -> Vec<T> {
    let mut sett = HashSet::new();
    verdier
        .into_iter()
        .filter(|verdi| sett.insert(*verdi))
        .collect()
}
