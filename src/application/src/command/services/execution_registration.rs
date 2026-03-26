use anyhow::Result;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::plan::{JournalpostType, Utsending};
use domain::eksekvering::typer::command_metadata;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::JournalpostCommon;
use lib_schemas::skuffen::command::sak::{AvsluttSak, OpprettSak};
use lib_schemas::skuffen::dokument::Dokument;
use lib_schemas::skuffen::query::queries::SakKey;
use uuid::Uuid;

use crate::command::ports::execution_registration_port::{
    DokumentStateRegistration, EksekveringssystemRegistration, JournalpostStateRegistration,
    SakStateRegistration,
};
use crate::command::ports::execution_snapshot_port::{
    DokumentState, JournalpostState, SakState, SakStatus,
};
use crate::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSakRegistration {
    pub sak_id: SkuffenSakId,
    pub state: SakState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedJournalpostRegistration {
    pub journalpost_id: SkuffenJournalpostId,
    pub sak_id: SkuffenSakId,
    pub state: JournalpostState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDokumentRegistration {
    pub dokument_id: SkuffenDokumentId,
    pub journalpost_id: SkuffenJournalpostId,
    pub state: DokumentState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedRegistration {
    pub sak: Option<ResolvedSakRegistration>,
    pub journalpost: Option<ResolvedJournalpostRegistration>,
    pub dokumenter: Vec<ResolvedDokumentRegistration>,
}

impl ResolvedRegistration {
    pub(crate) fn fra_envelope(
        envelope: &CommandEnvelope<Command>,
        sak: Option<ResolvedSakRegistration>,
        journalpost: Option<ResolvedJournalpostRegistration>,
        dokumenter: Vec<ResolvedDokumentRegistration>,
    ) -> Self {
        let _ = command_metadata(&envelope.payload);
        Self {
            sak,
            journalpost,
            dokumenter,
        }
    }

    pub(crate) fn til_eksekveringssystem_registrering(&self) -> EksekveringssystemRegistration {
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
            dokumenter: self
                .dokumenter
                .iter()
                .map(|dokument| DokumentStateRegistration {
                    dokument_id: dokument.dokument_id,
                    journalpost_id: dokument.journalpost_id,
                    state: dokument.state.clone(),
                })
                .collect(),
        }
    }

    pub(crate) fn sak_id(&self) -> Option<SkuffenSakId> {
        self.sak.as_ref().map(|sak| sak.sak_id)
    }

    pub(crate) fn journalpost_id(&self) -> Option<SkuffenJournalpostId> {
        self.journalpost
            .as_ref()
            .map(|journalpost| journalpost.journalpost_id)
    }
}

pub(crate) async fn resolve_registration(
    id_mapping_repo: &dyn IdMappingRepository,
    envelope: &CommandEnvelope<Command>,
) -> Result<ResolvedRegistration> {
    match &envelope.payload {
        Command::OpprettSak(cmd) => Ok(ResolvedRegistration::fra_envelope(
            envelope,
            Some(resolve_opprett_sak_registration(id_mapping_repo, cmd).await?),
            None,
            Vec::new(),
        )),
        Command::AvsluttSak(cmd) => Ok(ResolvedRegistration::fra_envelope(
            envelope,
            Some(resolve_avslutt_sak_registration(id_mapping_repo, cmd).await?),
            None,
            Vec::new(),
        )),
        Command::OpprettInngåendeJournalpost(cmd) => {
            resolve_journalpost_registration(
                id_mapping_repo,
                envelope,
                &cmd.felles,
                JournalpostType::Inngaende,
                None,
            )
            .await
        }
        Command::OpprettUtgåendeJournalpost(cmd) => {
            resolve_journalpost_registration(
                id_mapping_repo,
                envelope,
                &cmd.felles,
                JournalpostType::Utgaaende,
                Some(Utsending::UtenUtsending),
            )
            .await
        }
        Command::OpprettInterntNotatJournalpost(cmd) => {
            resolve_journalpost_registration(
                id_mapping_repo,
                envelope,
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
    envelope: &CommandEnvelope<Command>,
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

    let dokumenter =
        resolve_dokument_registrationer(id_mapping_repo, journalpost_id, &felles.dokumenter)
            .await?;

    Ok(ResolvedRegistration::fra_envelope(
        envelope,
        Some(sak.clone()),
        Some(ResolvedJournalpostRegistration {
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
        dokumenter,
    ))
}

async fn resolve_dokument_registrationer(
    id_mapping_repo: &dyn IdMappingRepository,
    journalpost_id: SkuffenJournalpostId,
    dokumenter: &[Dokument],
) -> Result<Vec<ResolvedDokumentRegistration>> {
    let mut resolved = Vec::with_capacity(dokumenter.len());

    for dokument in dokumenter {
        let dokument_id = id_mapping_repo
            .hent_dokument_id_fra_mapping(dokument.client_reference)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Fant ikke skuffen_id for dokument client_reference {}",
                    dokument.client_reference
                )
            })?;

        resolved.push(ResolvedDokumentRegistration {
            dokument_id,
            journalpost_id,
            state: DokumentState {
                lagt_til: false,
                irrecoverable_feil: false,
            },
        });
    }

    Ok(resolved)
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
