use anyhow::Result;
use domain::command::Command as DomainCommand;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::typer::CommandTypeCode;
use uuid::Uuid;

use crate::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};
use crate::command::{
    AvsluttSakCommand, Command as ApplicationCommand, CommandEnvelope, Dokument, JournalpostCommon,
    OpprettSakCommand, SakKey, SettSaksansvarligCommand,
};

/// Provenance origin for a resolved sak id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SakResolutionOrigin {
    /// Sak resolved from client reference (caller-created).
    ClientReference,
    /// Sak resolved from ArkivId (archive-validated); saksnummer is the archive case number.
    ArkivId { saksnummer: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSakId {
    pub sak_id: SkuffenSakId,
    pub origin: SakResolutionOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedJournalpostId {
    pub journalpost_id: SkuffenJournalpostId,
    pub sak_id: SkuffenSakId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDokumentId {
    pub dokument_id: SkuffenDokumentId,
    pub journalpost_id: SkuffenJournalpostId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedCommandIds {
    pub sak: Option<ResolvedSakId>,
    pub journalpost: Option<ResolvedJournalpostId>,
    pub dokumenter: Vec<ResolvedDokumentId>,
}

impl ResolvedCommandIds {
    pub(crate) fn new(
        sak: Option<ResolvedSakId>,
        journalpost: Option<ResolvedJournalpostId>,
        dokumenter: Vec<ResolvedDokumentId>,
    ) -> Self {
        Self {
            sak,
            journalpost,
            dokumenter,
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

pub(crate) fn domain_command_for_type(
    command_type: CommandTypeCode,
    sak_id: SkuffenSakId,
    journalpost_id: Option<SkuffenJournalpostId>,
) -> Result<DomainCommand> {
    match command_type {
        CommandTypeCode::OpprettSak => Ok(DomainCommand::OpprettSak { sak_id }),
        CommandTypeCode::AvsluttSak => Ok(DomainCommand::AvsluttSak { sak_id }),
        CommandTypeCode::SettSaksansvarlig => Ok(DomainCommand::SettSaksansvarlig { sak_id }),
        CommandTypeCode::OpprettInngaaendeJournalpost => {
            Ok(DomainCommand::OpprettInngaaendeJournalpost {
                sak_id,
                journalpost_id: journalpost_id.ok_or_else(|| {
                    anyhow::anyhow!("Mangler journalpost_id for journalpost-kommando")
                })?,
            })
        }
        CommandTypeCode::OpprettUtgaaendeJournalpost => {
            Ok(DomainCommand::OpprettUtgaaendeJournalpost {
                sak_id,
                journalpost_id: journalpost_id.ok_or_else(|| {
                    anyhow::anyhow!("Mangler journalpost_id for journalpost-kommando")
                })?,
            })
        }
        CommandTypeCode::OpprettInterntNotatJournalpost => {
            Ok(DomainCommand::OpprettInterntNotatJournalpost {
                sak_id,
                journalpost_id: journalpost_id.ok_or_else(|| {
                    anyhow::anyhow!("Mangler journalpost_id for journalpost-kommando")
                })?,
            })
        }
    }
}

pub(crate) async fn resolve_command_ids(
    id_mapping_repo: &dyn IdMappingRepository,
    envelope: &CommandEnvelope<ApplicationCommand>,
) -> Result<ResolvedCommandIds> {
    match &envelope.payload {
        ApplicationCommand::OpprettSak(cmd) => Ok(ResolvedCommandIds::new(
            Some(resolve_opprett_sak_id(id_mapping_repo, cmd).await?),
            None,
            Vec::new(),
        )),
        ApplicationCommand::AvsluttSak(cmd) => Ok(ResolvedCommandIds::new(
            Some(resolve_avslutt_sak_id(id_mapping_repo, cmd).await?),
            None,
            Vec::new(),
        )),
        ApplicationCommand::SettSaksansvarlig(cmd) => Ok(ResolvedCommandIds::new(
            Some(resolve_sett_saksansvarlig_id(id_mapping_repo, cmd).await?),
            None,
            Vec::new(),
        )),
        ApplicationCommand::OpprettInngaaendeJournalpost(cmd) => {
            resolve_journalpost_ids(id_mapping_repo, cmd.felles()).await
        }
        ApplicationCommand::OpprettUtgaaendeJournalpost(cmd) => {
            resolve_journalpost_ids(id_mapping_repo, cmd.felles()).await
        }
        ApplicationCommand::OpprettInterntNotatJournalpost(cmd) => {
            resolve_journalpost_ids(id_mapping_repo, cmd.felles()).await
        }
    }
}

async fn resolve_opprett_sak_id(
    id_mapping_repo: &dyn IdMappingRepository,
    command: &OpprettSakCommand,
) -> Result<ResolvedSakId> {
    Ok(ResolvedSakId {
        sak_id: resolve_skuffen_sak_id_for_client_reference(
            id_mapping_repo,
            command.client_reference,
        )
        .await?,
        origin: SakResolutionOrigin::ClientReference,
    })
}

async fn resolve_avslutt_sak_id(
    id_mapping_repo: &dyn IdMappingRepository,
    command: &AvsluttSakCommand,
) -> Result<ResolvedSakId> {
    resolve_sak_id(id_mapping_repo, &command.sak_key).await
}

async fn resolve_sett_saksansvarlig_id(
    id_mapping_repo: &dyn IdMappingRepository,
    command: &SettSaksansvarligCommand,
) -> Result<ResolvedSakId> {
    resolve_sak_id(id_mapping_repo, &command.sak_key).await
}

async fn resolve_journalpost_ids(
    id_mapping_repo: &dyn IdMappingRepository,
    felles: &JournalpostCommon,
) -> Result<ResolvedCommandIds> {
    let sak = resolve_sak_id(id_mapping_repo, &felles.sak_key).await?;
    let journalpost_id = resolve_skuffen_journalpost_id_for_client_reference(
        id_mapping_repo,
        felles.client_reference,
    )
    .await?;

    let dokumenter =
        resolve_dokument_ids(id_mapping_repo, journalpost_id, &felles.dokumenter).await?;

    Ok(ResolvedCommandIds::new(
        Some(sak.clone()),
        Some(ResolvedJournalpostId {
            journalpost_id,
            sak_id: sak.sak_id,
        }),
        dokumenter,
    ))
}

async fn resolve_dokument_ids(
    id_mapping_repo: &dyn IdMappingRepository,
    journalpost_id: SkuffenJournalpostId,
    dokumenter: &[Dokument],
) -> Result<Vec<ResolvedDokumentId>> {
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

        resolved.push(ResolvedDokumentId {
            dokument_id,
            journalpost_id,
        });
    }

    Ok(resolved)
}

async fn resolve_sak_id(
    id_mapping_repo: &dyn IdMappingRepository,
    sak_key: &SakKey,
) -> Result<ResolvedSakId> {
    match sak_key {
        SakKey::ClientReference(client_reference) => Ok(ResolvedSakId {
            sak_id: resolve_skuffen_sak_id_for_client_reference(id_mapping_repo, *client_reference)
                .await?,
            origin: SakResolutionOrigin::ClientReference,
        }),
        SakKey::ArkivId(saksnummer) => Ok(ResolvedSakId {
            sak_id: id_mapping_repo
                .hent_eller_opprett_skuffen_id_for_arkiv_id(
                    MappingEntityType::Sak,
                    saksnummer.as_str(),
                )
                .await?,
            origin: SakResolutionOrigin::ArkivId {
                saksnummer: saksnummer.to_string(),
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
