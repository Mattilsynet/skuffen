use anyhow::Result;
use domain::eksekvering::execution::{Kjorbarhet, Ventegrunn};
use domain::eksekvering::plan::JournalpostType;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};

use crate::command::ports::execution_registration_port::EksekveringssystemRegistration;
use crate::command::ports::execution_snapshot_port::{EksekveringSnapshotRepository, SakState};

pub struct EksekveringsklarhetVurderer {}

impl Default for EksekveringsklarhetVurderer {
    fn default() -> Self {
        Self::new()
    }
}

impl EksekveringsklarhetVurderer {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn vurder(
        &self,
        snapshot_repo: &dyn EksekveringSnapshotRepository,
        registration: &EksekveringssystemRegistration,
        envelope: &CommandEnvelope<Command>,
        sak_id: Option<domain::eksekvering::id::SkuffenSakId>,
        journalpost_id: Option<domain::eksekvering::id::SkuffenJournalpostId>,
    ) -> Result<Kjorbarhet> {
        match &envelope.payload {
            Command::OpprettSak(_) => Ok(Kjorbarhet::Klar),
            Command::OpprettInngåendeJournalpost(_)
            | Command::OpprettUtgåendeJournalpost(_)
            | Command::OpprettInterntNotatJournalpost(_) => {
                self.vurder_journalpost_kommando(snapshot_repo, registration, sak_id)
                    .await
            }
            Command::AvsluttSak(_) => {
                self.vurder_avslutt_sak(snapshot_repo, registration, sak_id, journalpost_id)
                    .await
            }
        }
    }

    async fn vurder_journalpost_kommando(
        &self,
        snapshot_repo: &dyn EksekveringSnapshotRepository,
        registration: &EksekveringssystemRegistration,
        sak_id: Option<domain::eksekvering::id::SkuffenSakId>,
    ) -> Result<Kjorbarhet> {
        let Some(sak_id) = sak_id else {
            return Ok(Kjorbarhet::Feil {
                detalj: "Mangler sak_id for journalpostkommando".to_string(),
            });
        };

        let Some(sak_state) = hent_sak_state_med_seed(snapshot_repo, registration, sak_id).await?
        else {
            return Ok(Kjorbarhet::Venter {
                grunn: Ventegrunn::SakOpprettet { sak_id },
                detalj: "Sak finnes ikke i skuffen-state".to_string(),
            });
        };

        if matches!(
            sak_state.status,
            crate::command::ports::execution_snapshot_port::SakStatus::Avsluttet
        ) {
            return Ok(Kjorbarhet::Feil {
                detalj: "Kan ikke opprette journalpost pa avsluttet sak".to_string(),
            });
        }

        if !sak_state.opprettet {
            return Ok(Kjorbarhet::Venter {
                grunn: Ventegrunn::SakOpprettet { sak_id },
                detalj: "Sak finnes ikke i skuffen-state".to_string(),
            });
        }

        if sak_state.saksnummer.is_none() {
            return Ok(Kjorbarhet::Venter {
                grunn: Ventegrunn::SaksnummerTildelt { sak_id },
                detalj: "Saksnummer mangler".to_string(),
            });
        }

        Ok(Kjorbarhet::Klar)
    }

    async fn vurder_avslutt_sak(
        &self,
        snapshot_repo: &dyn EksekveringSnapshotRepository,
        registration: &EksekveringssystemRegistration,
        sak_id: Option<domain::eksekvering::id::SkuffenSakId>,
        _journalpost_id: Option<domain::eksekvering::id::SkuffenJournalpostId>,
    ) -> Result<Kjorbarhet> {
        let Some(sak_id) = sak_id else {
            return Ok(Kjorbarhet::Feil {
                detalj: "Mangler sak_id for avslutt sak".to_string(),
            });
        };

        let Some(sak_state) = hent_sak_state_med_seed(snapshot_repo, registration, sak_id).await?
        else {
            return Ok(Kjorbarhet::Venter {
                grunn: Ventegrunn::SakOpprettet { sak_id },
                detalj: "Kan ikke avslutte sak: saken finnes ikke i state".to_string(),
            });
        };

        if !sak_state.opprettet {
            return Ok(Kjorbarhet::Venter {
                grunn: Ventegrunn::SakOpprettet { sak_id },
                detalj: "Kan ikke avslutte sak: saken finnes ikke i state".to_string(),
            });
        }

        if sak_state.saksnummer.is_none() {
            return Ok(Kjorbarhet::Venter {
                grunn: Ventegrunn::SaksnummerTildelt { sak_id },
                detalj: "Kan ikke avslutte sak: saksnummer mangler i state".to_string(),
            });
        }

        let journalposter = snapshot_repo.hent_journalposter_for_sak(sak_id).await?;
        if journalposter
            .iter()
            .any(|journalpost| journalpost.har_feilede_dokumenter)
        {
            return Ok(Kjorbarhet::Feil {
                detalj: "Kan ikke avslutte sak: minst ett dokument pa en journalpost har feilet"
                    .to_string(),
            });
        }

        if journalposter.iter().any(journalpost_er_uferdig) {
            return Ok(Kjorbarhet::Venter {
                grunn: Ventegrunn::SakHarUferdigeJournalposter { sak_id },
                detalj: "Kan ikke avslutte sak: saken har uferdige journalposter".to_string(),
            });
        }

        Ok(Kjorbarhet::Klar)
    }
}

async fn hent_sak_state_med_seed(
    snapshot_repo: &dyn EksekveringSnapshotRepository,
    registration: &EksekveringssystemRegistration,
    sak_id: domain::eksekvering::id::SkuffenSakId,
) -> Result<Option<SakState>> {
    if let Some(state) = snapshot_repo.hent_sak_state(sak_id).await? {
        return Ok(Some(state));
    }

    Ok(registration
        .sak
        .as_ref()
        .filter(|sak| sak.sak_id == sak_id)
        .map(|sak| sak.state.clone()))
}

fn journalpost_er_uferdig(
    journalpost: &crate::command::ports::execution_snapshot_port::JournalpostState,
) -> bool {
    match journalpost.journalposttype {
        JournalpostType::Inngaende => !journalpost.journalfoert || !journalpost.avskrevet,
        JournalpostType::Utgaaende | JournalpostType::InterntNotat => !journalpost.journalfoert,
    }
}
