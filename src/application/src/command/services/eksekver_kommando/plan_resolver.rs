use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::plan::{EksekveringsPlan, JournalpostPlan, Steg};
use domain::eksekvering::typer::EksekveringFeil;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::query::queries::SakKey;
use uuid::Uuid;

use crate::command::ports::id_mapping_port::MappingEntityType;

use super::resolved_plan::{ResolvedDocument, ResolvedJournalpostPlan, ResolvedPlan, ResolvedStep};
use super::EksekverKommandoService;

impl EksekverKommandoService {
    pub(super) async fn resolve_plan(
        &self,
        _envelope: &CommandEnvelope<Command>,
        plan: EksekveringsPlan,
    ) -> Result<ResolvedPlan, EksekveringFeil> {
        let mut resolved_steg = Vec::with_capacity(plan.steg.len());

        for steg in plan.steg {
            resolved_steg.push(self.resolve_step(steg).await?);
        }

        Ok(ResolvedPlan {
            steg: resolved_steg,
        })
    }

    async fn resolve_step(&self, steg: Steg) -> Result<ResolvedStep, EksekveringFeil> {
        match steg {
            Steg::OpprettSak { sak_id } => Ok(ResolvedStep::OpprettSak {
                sak_client_reference: sak_id,
                sak_id: self
                    .resolve_sak_entity_id_for_client_reference(sak_id)
                    .await?,
            }),
            Steg::OpprettJournalpost { plan } => Ok(ResolvedStep::OpprettJournalpost {
                plan: self.resolve_journalpost_plan(plan).await?,
            }),
            Steg::LeggTilDokument {
                journalpost_id,
                dokument_id,
            } => Ok(ResolvedStep::LeggTilDokument {
                journalpost_id: self
                    .resolve_journalpost_entity_id_for_client_reference(journalpost_id)
                    .await?,
                dokument_id: self
                    .resolve_dokument_entity_id_for_client_reference(dokument_id)
                    .await?,
                dokument_client_reference: dokument_id,
            }),
            Steg::Journalfoer { journalpost_id } => Ok(ResolvedStep::Journalfoer {
                journalpost_id: self
                    .resolve_journalpost_entity_id_for_client_reference(journalpost_id)
                    .await?,
            }),
            Steg::Avskriv { journalpost_id } => Ok(ResolvedStep::Avskriv {
                journalpost_id: self
                    .resolve_journalpost_entity_id_for_client_reference(journalpost_id)
                    .await?,
            }),
            Steg::AvsluttSak { sak_key } => Ok(ResolvedStep::AvsluttSak {
                sak_id: self.resolve_sak_id(sak_key).await?,
            }),
        }
    }

    async fn resolve_journalpost_plan(
        &self,
        plan: JournalpostPlan,
    ) -> Result<ResolvedJournalpostPlan, EksekveringFeil> {
        let sak_id = self.resolve_sak_id(plan.sak_key.clone()).await?;
        let journalpost_id = self
            .resolve_journalpost_entity_id_for_client_reference(plan.journalpost_id)
            .await?;

        let mut dokumenter = Vec::with_capacity(plan.dokumenter.len());
        for client_reference in plan.dokumenter {
            dokumenter.push(ResolvedDocument {
                dokument_id: self
                    .resolve_dokument_entity_id_for_client_reference(client_reference)
                    .await?,
                client_reference,
            });
        }

        Ok(ResolvedJournalpostPlan {
            journalpost_id,
            journalpost_client_reference: plan.journalpost_id,
            sak_id,
            utsending: plan.utsending,
            dokumenter,
        })
    }

    pub(super) async fn resolve_sak_id(
        &self,
        sak_key: SakKey,
    ) -> Result<SkuffenSakId, EksekveringFeil> {
        match sak_key {
            SakKey::ClientReference(client_reference) => {
                self.resolve_sak_entity_id_for_client_reference(client_reference)
                    .await
            }
            SakKey::ArkivId(saksnummer) => match self
                .id_mapping
                .hent_sak_id_fra_arkiv_id_i_mapping(saksnummer.as_str())
                .await
            {
                Ok(Some(skuffen_id)) => Ok(skuffen_id),
                Ok(None) => Err(EksekveringFeil::blocked(format!(
                    "Fant ikke skuffen_id for sak arkiv_id {}",
                    saksnummer.as_str()
                ))),
                Err(err) => Err(EksekveringFeil::recoverable(err.to_string())),
            },
        }
    }

    pub(super) async fn resolve_sak_entity_id_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<SkuffenSakId, EksekveringFeil> {
        self.id_mapping
            .hent_sak_id_fra_mapping(client_reference)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?
            .ok_or_else(|| {
                EksekveringFeil::recoverable(format!(
                    "Fant ikke skuffen_id for {} client_reference {}",
                    MappingEntityType::Sak.as_code(),
                    client_reference
                ))
            })
    }

    pub(super) async fn resolve_journalpost_entity_id_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<SkuffenJournalpostId, EksekveringFeil> {
        self.id_mapping
            .hent_journalpost_id_fra_mapping(client_reference)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?
            .ok_or_else(|| {
                EksekveringFeil::recoverable(format!(
                    "Fant ikke skuffen_id for {} client_reference {}",
                    MappingEntityType::Journalpost.as_code(),
                    client_reference
                ))
            })
    }

    pub(super) async fn resolve_dokument_entity_id_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<SkuffenDokumentId, EksekveringFeil> {
        self.id_mapping
            .hent_dokument_id_fra_mapping(client_reference)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?
            .ok_or_else(|| {
                EksekveringFeil::recoverable(format!(
                    "Fant ikke skuffen_id for {} client_reference {}",
                    MappingEntityType::Dokument.as_code(),
                    client_reference
                ))
            })
    }
}
