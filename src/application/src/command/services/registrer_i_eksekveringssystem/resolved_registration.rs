use anyhow::Result;
use domain::eksekvering::id::{SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::plan::{JournalpostType, Utsending};
use lib_schemas::skuffen::command::commands::Command;
use lib_schemas::skuffen::command::journalpost::JournalpostCommon;
use lib_schemas::skuffen::command::sak::{AvsluttSak, OpprettSak};
use lib_schemas::skuffen::query::queries::SakKey;
use uuid::Uuid;

use crate::command::ports::eksekvering_state_port::{
    EksekveringssystemRegistration, JournalpostState, JournalpostStateRegistration, SakState,
    SakStateRegistration, SakStatus,
};
use crate::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedSakRegistration {
    pub sak_id: SkuffenSakId,
    pub state: SakState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedJournalpostRegistration {
    pub journalpost_id: SkuffenJournalpostId,
    pub sak_id: SkuffenSakId,
    pub state: JournalpostState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedRegistration {
    pub sak: Option<ResolvedSakRegistration>,
    pub journalpost: Option<ResolvedJournalpostRegistration>,
}

impl ResolvedRegistration {
    pub(super) fn til_eksekveringssystem_registrering(&self) -> EksekveringssystemRegistration {
        EksekveringssystemRegistration {
            sak: self.sak.as_ref().map(|sak| SakStateRegistration {
                sak_id: sak.sak_id,
                state: sak.state.clone(),
            }),
            journalpost: self.journalpost.as_ref().map(|journalpost| {
                JournalpostStateRegistration {
                    journalpost_id: journalpost.journalpost_id,
                    sak_id: journalpost.sak_id,
                    state: journalpost.state.clone(),
                }
            }),
        }
    }
}

pub(super) async fn resolve_registration(
    id_mapping_repo: &dyn IdMappingRepository,
    command: &Command,
) -> Result<ResolvedRegistration> {
    match command {
        Command::OpprettSak(cmd) => Ok(ResolvedRegistration {
            sak: Some(resolve_opprett_sak_registration(id_mapping_repo, cmd).await?),
            journalpost: None,
        }),
        Command::AvsluttSak(cmd) => Ok(ResolvedRegistration {
            sak: Some(resolve_avslutt_sak_registration(id_mapping_repo, cmd).await?),
            journalpost: None,
        }),
        Command::OpprettInngåendeJournalpost(cmd) => {
            resolve_journalpost_registration(
                id_mapping_repo,
                &cmd.felles,
                JournalpostType::Inngaende,
                None,
            )
            .await
        }
        Command::OpprettUtgåendeJournalpost(cmd) => {
            resolve_journalpost_registration(
                id_mapping_repo,
                &cmd.felles,
                JournalpostType::Utgaaende,
                Some(Utsending::UtenUtsending),
            )
            .await
        }
        Command::OpprettInterntNotatJournalpost(cmd) => {
            resolve_journalpost_registration(
                id_mapping_repo,
                &cmd.felles,
                JournalpostType::InterntNotat,
                None,
            )
            .await
        }
    }
}

async fn resolve_opprett_sak_registration(
    id_mapping_repo: &dyn IdMappingRepository,
    command: &OpprettSak,
) -> Result<ResolvedSakRegistration> {
    Ok(ResolvedSakRegistration {
        sak_id: resolve_skuffen_sak_id_for_client_reference(
            id_mapping_repo,
            command.client_reference,
        )
        .await?,
        state: SakState {
            status: SakStatus::UnderBehandling,
            opprettet: false,
            saksnummer: None,
        },
    })
}

async fn resolve_avslutt_sak_registration(
    id_mapping_repo: &dyn IdMappingRepository,
    command: &AvsluttSak,
) -> Result<ResolvedSakRegistration> {
    resolve_sak_registration(id_mapping_repo, &command.sak_key).await
}

async fn resolve_journalpost_registration(
    id_mapping_repo: &dyn IdMappingRepository,
    felles: &JournalpostCommon,
    journalpost_type: JournalpostType,
    utsending: Option<Utsending>,
) -> Result<ResolvedRegistration> {
    let sak = resolve_sak_registration(id_mapping_repo, &felles.sak_key).await?;
    let journalpost_id = resolve_skuffen_journalpost_id_for_client_reference(
        id_mapping_repo,
        felles.client_reference,
    )
    .await?;

    Ok(ResolvedRegistration {
        sak: Some(sak.clone()),
        journalpost: Some(ResolvedJournalpostRegistration {
            journalpost_id,
            sak_id: sak.sak_id,
            state: JournalpostState {
                journalfoert: false,
                avskrevet: false,
                ekspedert: false,
                har_feilede_dokumenter: false,
                med_utsending: matches!(utsending, Some(Utsending::MedUtsending)),
                journalposttype: journalpost_type,
                journalpostnummer: None,
            },
        }),
    })
}

async fn resolve_sak_registration(
    id_mapping_repo: &dyn IdMappingRepository,
    sak_key: &SakKey,
) -> Result<ResolvedSakRegistration> {
    match sak_key {
        SakKey::ClientReference(client_reference) => Ok(ResolvedSakRegistration {
            sak_id: resolve_skuffen_sak_id_for_client_reference(id_mapping_repo, *client_reference)
                .await?,
            state: SakState {
                status: SakStatus::UnderBehandling,
                opprettet: false,
                saksnummer: None,
            },
        }),
        SakKey::ArkivId(saksnummer) => Ok(ResolvedSakRegistration {
            sak_id: id_mapping_repo
                .hent_eller_opprett_skuffen_id_for_arkiv_id(
                    MappingEntityType::Sak,
                    saksnummer.as_str(),
                )
                .await?,
            state: SakState {
                status: SakStatus::UnderBehandling,
                opprettet: true,
                saksnummer: Some(saksnummer.as_str().to_string()),
            },
        }),
    }
}

async fn resolve_skuffen_sak_id_for_client_reference(
    id_mapping_repo: &dyn IdMappingRepository,
    client_reference: Uuid,
) -> Result<SkuffenSakId> {
    match id_mapping_repo
        .hent_sak_id_fra_mapping(client_reference)
        .await?
    {
        Some(skuffen_id) => Ok(skuffen_id),
        None => Err(anyhow::anyhow!(
            "Fant ikke skuffen_id for sak client_reference {client_reference}"
        )),
    }
}

async fn resolve_skuffen_journalpost_id_for_client_reference(
    id_mapping_repo: &dyn IdMappingRepository,
    client_reference: Uuid,
) -> Result<SkuffenJournalpostId> {
    match id_mapping_repo
        .hent_journalpost_id_fra_mapping(client_reference)
        .await?
    {
        Some(skuffen_id) => Ok(skuffen_id),
        None => Err(anyhow::anyhow!(
            "Fant ikke skuffen_id for journalpost client_reference {client_reference}"
        )),
    }
}
