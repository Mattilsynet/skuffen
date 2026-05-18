use anyhow::Result;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::CommandTarget;
use domain::eksekvering::typer::CommandTypeCode;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::JournalpostCommon;
use lib_schemas::skuffen::command::sak::{AvsluttSak, OpprettSak};
use lib_schemas::skuffen::dokument::Dokument;
use lib_schemas::skuffen::query::queries::SakKey;
use uuid::Uuid;

use crate::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedSakRegistration {
    pub sak_id: SkuffenSakId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedJournalpostRegistration {
    pub journalpost_id: SkuffenJournalpostId,
    pub sak_id: SkuffenSakId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDokumentRegistration {
    pub dokument_id: SkuffenDokumentId,
    pub journalpost_id: SkuffenJournalpostId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedRegistration {
    pub sak: Option<ResolvedSakRegistration>,
    pub journalpost: Option<ResolvedJournalpostRegistration>,
    pub dokumenter: Vec<ResolvedDokumentRegistration>,
}

impl ResolvedRegistration {
    pub(crate) fn new(
        sak: Option<ResolvedSakRegistration>,
        journalpost: Option<ResolvedJournalpostRegistration>,
        dokumenter: Vec<ResolvedDokumentRegistration>,
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

pub(crate) fn command_target_for_type(
    command_type: CommandTypeCode,
    journalpost_id: Option<SkuffenJournalpostId>,
) -> Result<CommandTarget> {
    match command_type {
        CommandTypeCode::OpprettSak
        | CommandTypeCode::AvsluttSak
        | CommandTypeCode::SettSaksansvarlig => Ok(CommandTarget::Sak),
        CommandTypeCode::OpprettInngaaendeJournalpost
        | CommandTypeCode::OpprettUtgaaendeJournalpost
        | CommandTypeCode::OpprettInterntNotatJournalpost => journalpost_id
            .map(CommandTarget::Journalpost)
            .ok_or_else(|| anyhow::anyhow!("Mangler journalpost_id for journalpost-kommando")),
    }
}

pub(crate) async fn resolve_registration(
    id_mapping_repo: &dyn IdMappingRepository,
    envelope: &CommandEnvelope<Command>,
) -> Result<ResolvedRegistration> {
    match &envelope.payload {
        Command::OpprettSak(cmd) => Ok(ResolvedRegistration::new(
            Some(resolve_opprett_sak_registration(id_mapping_repo, cmd).await?),
            None,
            Vec::new(),
        )),
        Command::AvsluttSak(cmd) => Ok(ResolvedRegistration::new(
            Some(resolve_avslutt_sak_registration(id_mapping_repo, cmd).await?),
            None,
            Vec::new(),
        )),
        Command::SettSaksansvarlig(cmd) => Ok(ResolvedRegistration::new(
            Some(resolve_sett_saksansvarlig_registration(id_mapping_repo, cmd).await?),
            None,
            Vec::new(),
        )),
        Command::OpprettInngåendeJournalpost(cmd) => {
            resolve_journalpost_registration(id_mapping_repo, &cmd.felles).await
        }
        Command::OpprettUtgåendeJournalpost(cmd) => {
            resolve_journalpost_registration(id_mapping_repo, &cmd.felles).await
        }
        Command::OpprettInterntNotatJournalpost(cmd) => {
            resolve_journalpost_registration(id_mapping_repo, &cmd.felles).await
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
    })
}

async fn resolve_avslutt_sak_registration(
    id_mapping_repo: &dyn IdMappingRepository,
    command: &AvsluttSak,
) -> Result<ResolvedSakRegistration> {
    resolve_sak_registration(id_mapping_repo, &command.sak_key).await
}

async fn resolve_sett_saksansvarlig_registration(
    id_mapping_repo: &dyn IdMappingRepository,
    command: &lib_schemas::skuffen::command::sak::SettSaksansvarlig,
) -> Result<ResolvedSakRegistration> {
    resolve_sak_registration(id_mapping_repo, &command.sak_key).await
}

async fn resolve_journalpost_registration(
    id_mapping_repo: &dyn IdMappingRepository,
    felles: &JournalpostCommon,
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

    Ok(ResolvedRegistration::new(
        Some(sak.clone()),
        Some(ResolvedJournalpostRegistration {
            journalpost_id,
            sak_id: sak.sak_id,
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
        }),
        SakKey::ArkivId(saksnummer) => Ok(ResolvedSakRegistration {
            sak_id: id_mapping_repo
                .hent_eller_opprett_skuffen_id_for_arkiv_id(
                    MappingEntityType::Sak,
                    saksnummer.as_str(),
                )
                .await?,
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
