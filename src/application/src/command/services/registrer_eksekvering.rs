use anyhow::Result;
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::query::queries::SakKey as DtoSakKey;

use crate::command::ports::eksekvering_state_port::{
    EksekveringStateRepository, JournalpostState, SakState, SakStatus,
};
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::registrer_eksekvering_port::RegistrerEksekveringUseCase;
use domain::eksekvering::plan::{EksekveringsPlan, JournalpostType, Steg, Utsending};

pub struct RegistrerEksekveringService {
    state_repo: Box<dyn EksekveringStateRepository>,
    id_mapping_repo: Box<dyn IdMappingRepository>,
}

impl RegistrerEksekveringService {
    pub fn new(
        state_repo: Box<dyn EksekveringStateRepository>,
        id_mapping_repo: Box<dyn IdMappingRepository>,
    ) -> Self {
        Self {
            state_repo,
            id_mapping_repo,
        }
    }

    async fn ensure_sak_state(&self, envelope: &CommandEnvelope<Command>) -> Result<()> {
        let plan = EksekveringsPlan::fra_command(&envelope.payload)
            .map_err(|err| anyhow::anyhow!(err.melding))?;

        if let Some(Steg::OpprettJournalpost { plan }) = plan.steg.first() {
            let (sak_id, sak_state) = match &plan.sak_key {
                DtoSakKey::ClientReference(sak_id) => (
                    *sak_id,
                    SakState {
                        status: SakStatus::UnderBehandling,
                        opprettet: false,
                        saksnummer: None,
                    },
                ),
                DtoSakKey::ArkivId(saksnummer) => {
                    let sak_id = self
                        .id_mapping_repo
                        .hent_eller_opprett_skuffen_id_for_arkiv_id("sak", saksnummer.as_str())
                        .await?;
                    (
                        sak_id,
                        SakState {
                            status: SakStatus::UnderBehandling,
                            opprettet: true,
                            saksnummer: Some(saksnummer.as_str().to_string()),
                        },
                    )
                }
            };

            if self.state_repo.hent_sak_state_fra_state(sak_id).await?.is_none() {
                self.state_repo.lagre_sak_state(sak_id, sak_state).await?;
            }

            if self
                .state_repo
                .hent_journalpost_state_fra_state(plan.journalpost_id)
                .await?
                .is_none()
            {
                let journalposttype = match plan.journalpost_type {
                    JournalpostType::Inngaende => 'I',
                    JournalpostType::Utgaaende => 'U',
                    JournalpostType::InterntNotat => 'X',
                };
                let journalpost_state = JournalpostState {
                    journalfoert: false,
                    avskrevet: false,
                    ekspedert: false,
                    har_feilede_dokumenter: false,
                    med_utsending: matches!(plan.utsending, Some(Utsending::MedUtsending)),
                    journalposttype,
                    journalpostnummer: None,
                };
                self.state_repo
                    .lagre_journalpost_state(plan.journalpost_id, sak_id, journalpost_state)
                    .await?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl RegistrerEksekveringUseCase for RegistrerEksekveringService {
    async fn handle(&self, envelope: &CommandEnvelope<Command>) -> Result<()> {
        self.ensure_sak_state(envelope).await?;
        self.state_repo.registrer_kommando(envelope).await
    }
}
