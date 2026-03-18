use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope, CommandStatus};
use lib_schemas::skuffen::query::queries::SakKey;
use std::fmt::Write;
use uuid::Uuid;

use crate::command::ports::eksekvering_port::{
    ArkivGateway, EksekveringKvitteringPublisher, EksekveringStatusPublisher,
    OpprettJournalpostResultat, Utsendingsvalg,
};
use crate::command::ports::eksekvering_state_port::{
    DokumentState, EksekveringStateRepository, JournalpostState, SakState, SakStatus,
};
use crate::command::ports::id_mapping_port::IdMappingRepository;
use domain::eksekvering::plan::{EksekveringsPlan, JournalpostType, Steg, Utsending};
use domain::eksekvering::typer::{status_event, EksekveringFeil, EksekveringFeiltype};

pub struct EksekverKommandoService {
    state_repo: Box<dyn EksekveringStateRepository>,
    arkiv_gateway: Box<dyn ArkivGateway>,
    status_publisher: Box<dyn EksekveringStatusPublisher>,
    done_publisher: Box<dyn EksekveringKvitteringPublisher>,
    id_mapping: Box<dyn IdMappingRepository>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionOutcome {
    Ok,
    Blocked { last_error: Option<String> },
    Retrying { last_error: Option<String> },
    Error { last_error: Option<String> },
}

#[derive(Debug, Clone)]
struct ExecutionStepResult {
    #[allow(dead_code)]
    reason: Option<String>,
}

impl ExecutionStepResult {
    fn completed() -> Self {
        Self { reason: None }
    }

    fn skipped(reason: impl Into<String>) -> Self {
        Self {
            reason: Some(reason.into()),
        }
    }
}

enum ExecutionGuard<T> {
    Proceed(T),
    Skip(ExecutionStepResult),
}

impl<T> ExecutionGuard<T> {
    fn proceed(value: T) -> Self {
        Self::Proceed(value)
    }

    fn skip(reason: impl Into<String>) -> Self {
        Self::Skip(ExecutionStepResult::skipped(reason))
    }
}

impl EksekverKommandoService {
    pub fn new(
        state_repo: Box<dyn EksekveringStateRepository>,
        arkiv_gateway: Box<dyn ArkivGateway>,
        status_publisher: Box<dyn EksekveringStatusPublisher>,
        done_publisher: Box<dyn EksekveringKvitteringPublisher>,
        id_mapping: Box<dyn IdMappingRepository>,
    ) -> Self {
        Self {
            state_repo,
            arkiv_gateway,
            status_publisher,
            done_publisher,
            id_mapping,
        }
    }

    pub async fn handle(
        &self,
        envelope: CommandEnvelope<Command>,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        self.status_publisher
            .publiser_status(status_event(&envelope, CommandStatus::Pending, None, None))
            .await?;

        let plan = match EksekveringsPlan::fra_command(&envelope.payload) {
            Ok(plan) => plan,
            Err(err) => {
                return self.avslutt_med_feil(&envelope, err).await;
            }
        };

        match self.execute_plan(&envelope, plan).await {
            Ok(()) => {
                let refs_message = self.build_reference_message(&envelope).await;
                self.status_publisher
                    .publiser_status(status_event(
                        &envelope,
                        CommandStatus::Ok,
                        refs_message,
                        None,
                    ))
                    .await?;
                let (subject, _) = domain::eksekvering::typer::done_subject(&envelope);
                self.done_publisher
                    .publiser_done(&subject, &envelope)
                    .await?;
                Ok(ExecutionOutcome::Ok)
            }
            Err(err) => self.avslutt_med_feil(&envelope, err).await,
        }
    }

    /// Executes the plan sequentially. Each step is state-gated and idempotent,
    /// so re-processing after a crash will skip completed work.
    async fn execute_plan(
        &self,
        envelope: &CommandEnvelope<Command>,
        plan: EksekveringsPlan,
    ) -> Result<(), EksekveringFeil> {
        for steg in plan.steg {
            let result = self.execute_step(envelope, steg).await?;
            self.observe_step_result(&result);
        }
        Ok(())
    }

    async fn execute_step(
        &self,
        envelope: &CommandEnvelope<Command>,
        steg: Steg,
    ) -> Result<ExecutionStepResult, EksekveringFeil> {
        match steg {
            Steg::OpprettSak { sak_id } => self.opprett_sak(envelope, sak_id).await,
            Steg::OpprettJournalpost { plan } => self.opprett_journalpost(envelope, plan).await,
            Steg::LeggTilDokument {
                journalpost_id,
                dokument_id,
            } => {
                self.legg_til_dokument(envelope, journalpost_id, dokument_id)
                    .await
            }
            Steg::Journalfoer { journalpost_id } => {
                self.journalfoer_journalpost(envelope, journalpost_id).await
            }
            Steg::Avskriv { journalpost_id } => {
                self.avskriv_journalpost(envelope, journalpost_id).await
            }
            Steg::AvsluttSak { sak_id } => self.avslutt_sak(envelope, sak_id).await,
        }
    }

    async fn opprett_sak(
        &self,
        envelope: &CommandEnvelope<Command>,
        sak_id: Uuid,
    ) -> Result<ExecutionStepResult, EksekveringFeil> {
        let _ = match self.guard_sak_ikke_opprettet(sak_id).await? {
            ExecutionGuard::Proceed(state) => state,
            ExecutionGuard::Skip(result) => return Ok(result),
        };

        let saksnummer = self
            .arkiv_gateway
            .opprett_sak(envelope)
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        let client_reference = match &envelope.payload {
            Command::OpprettSak(cmd) => cmd.client_reference,
            _ => sak_id,
        };
        self.id_mapping
            .oppdater_arkiv_id_for_client_reference(client_reference, saksnummer.clone())
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        let oppdatert = SakState {
            status: SakStatus::UnderBehandling,
            opprettet: true,
            saksnummer: Some(saksnummer.clone()),
        };
        self.state_repo
            .lagre_sak_state(sak_id, oppdatert)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        let _ = (sak_id, saksnummer);
        Ok(ExecutionStepResult::completed())
    }

    async fn opprett_journalpost(
        &self,
        envelope: &CommandEnvelope<Command>,
        plan: domain::eksekvering::plan::JournalpostPlan,
    ) -> Result<ExecutionStepResult, EksekveringFeil> {
        let sak_id = match plan.sak_key.clone() {
            SakKey::ClientReference(id) => id,
            SakKey::ArkivId(saksnummer) => self
                .id_mapping
                .ensure_arkiv_mapping("sak", saksnummer.as_str())
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?,
        };

        let _ = match self
            .guard_journalpost_sak(plan.journalpost_id, sak_id)
            .await?
        {
            ExecutionGuard::Proceed(state) => state,
            ExecutionGuard::Skip(result) => return Ok(result),
        };

        let utsending = plan.utsending.map(|u| match u {
            Utsending::MedUtsending => Utsendingsvalg::MedUtsending,
            Utsending::UtenUtsending => Utsendingsvalg::UtenUtsending,
        });

        let saksnummer = self
            .hent_saksnummer(plan.sak_key)
            .await
            .ok_or_else(|| EksekveringFeil::blocked("Saksnummer mangler"))?;

        let OpprettJournalpostResultat { journalpost_id } = self
            .arkiv_gateway
            .opprett_journalpost(envelope, saksnummer.as_str(), utsending)
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        let client_reference = match &envelope.payload {
            Command::OpprettInngåendeJournalpost(cmd) => cmd.felles.client_reference,
            Command::OpprettUtgåendeJournalpost(cmd) => cmd.felles.client_reference,
            Command::OpprettInterntNotatJournalpost(cmd) => cmd.felles.client_reference,
            _ => plan.journalpost_id,
        };
        self.id_mapping
            .oppdater_arkiv_id_for_client_reference(client_reference, journalpost_id.to_string())
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        let journalposttype = match plan.journalpost_type {
            JournalpostType::Inngaende => 'I',
            JournalpostType::Utgaaende => 'U',
            JournalpostType::InterntNotat => 'X',
        };

        let state = JournalpostState {
            journalfoert: false,
            avskrevet: false,
            ekspedert: false,
            har_feilede_dokumenter: false,
            med_utsending: matches!(plan.utsending, Some(Utsending::MedUtsending)),
            journalposttype,
            journalpostnummer: Some(journalpost_id),
        };
        self.state_repo
            .lagre_journalpost_state(plan.journalpost_id, sak_id, state)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        let _ = (plan.journalpost_id, journalpost_id);
        Ok(ExecutionStepResult::completed())
    }

    async fn legg_til_dokument(
        &self,
        envelope: &CommandEnvelope<Command>,
        journalpost_id: Uuid,
        dokument_id: Uuid,
    ) -> Result<ExecutionStepResult, EksekveringFeil> {
        let _ = match self.guard_journalpost_finnes(journalpost_id).await? {
            ExecutionGuard::Proceed(state) => state,
            ExecutionGuard::Skip(result) => return Ok(result),
        };

        let _ = match self.guard_dokument_ikke_lagt_til(dokument_id).await? {
            ExecutionGuard::Proceed(state) => state,
            ExecutionGuard::Skip(result) => return Ok(result),
        };

        let journalpostnummer = self
            .hent_journalpostnummer(journalpost_id)
            .await
            .ok_or_else(|| EksekveringFeil::blocked("Journalpostnummer mangler"))?;

        let resp = self
            .arkiv_gateway
            .legg_til_vedlegg(envelope, journalpostnummer, vec![dokument_id])
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        if let Some(Some(arkiv_id)) = resp.into_iter().next() {
            self.id_mapping
                .oppdater_arkiv_id_for_client_reference(dokument_id, arkiv_id.to_string())
                .await
                .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        }

        self.state_repo
            .lagre_dokument_state(
                dokument_id,
                journalpost_id,
                DokumentState {
                    lagt_til: true,
                    irrecoverable_feil: false,
                },
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(ExecutionStepResult::completed())
    }

    async fn journalfoer_journalpost(
        &self,
        _envelope: &CommandEnvelope<Command>,
        journalpost_id: Uuid,
    ) -> Result<ExecutionStepResult, EksekveringFeil> {
        let state = match self
            .guard_journalpost_kan_journalfores(journalpost_id)
            .await?
        {
            ExecutionGuard::Proceed(state) => state,
            ExecutionGuard::Skip(result) => return Ok(result),
        };

        let journalpostnummer = self
            .hent_journalpostnummer(journalpost_id)
            .await
            .ok_or_else(|| EksekveringFeil::blocked("Journalpostnummer mangler"))?;

        let (ny_status, journalfoert, ekspedert) = match state.journalposttype {
            'U' => {
                if state.med_utsending {
                    ("F", false, false)
                } else {
                    ("J", true, state.ekspedert)
                }
            }
            _ => ("J", true, state.ekspedert),
        };

        self.arkiv_gateway
            .sett_journalpost_status(journalpostnummer, ny_status)
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        self.state_repo
            .lagre_journalpost_state(
                journalpost_id,
                Uuid::nil(),
                JournalpostState {
                    journalfoert,
                    ekspedert,
                    ..state
                },
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(ExecutionStepResult::completed())
    }

    async fn avskriv_journalpost(
        &self,
        _envelope: &CommandEnvelope<Command>,
        journalpost_id: Uuid,
    ) -> Result<ExecutionStepResult, EksekveringFeil> {
        let state = match self.guard_journalpost_kan_avskrives(journalpost_id).await? {
            ExecutionGuard::Proceed(state) => state,
            ExecutionGuard::Skip(result) => return Ok(result),
        };

        let journalpostnummer = self
            .hent_journalpostnummer(journalpost_id)
            .await
            .ok_or_else(|| EksekveringFeil::blocked("Journalpostnummer mangler"))?;

        self.arkiv_gateway
            .avskriv_journalpost(journalpostnummer, "TE")
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        self.state_repo
            .lagre_journalpost_state(
                journalpost_id,
                Uuid::nil(),
                JournalpostState {
                    avskrevet: true,
                    ..state
                },
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(ExecutionStepResult::completed())
    }

    async fn avslutt_sak(
        &self,
        _envelope: &CommandEnvelope<Command>,
        sak_id: Uuid,
    ) -> Result<ExecutionStepResult, EksekveringFeil> {
        let _ = match self.guard_sak_kan_avsluttes(sak_id).await? {
            ExecutionGuard::Proceed(state) => state,
            ExecutionGuard::Skip(result) => return Ok(result),
        };

        let saksnummer = self
            .hent_saksnummer(SakKey::ClientReference(sak_id))
            .await
            .ok_or_else(|| EksekveringFeil::blocked("Saksnummer mangler"))?;

        self.arkiv_gateway
            .avslutt_sak(saksnummer.as_str())
            .await
            .map_err(|err| self.map_arkiv_feil(err))?;

        self.state_repo
            .lagre_sak_state(
                sak_id,
                SakState {
                    status: SakStatus::Avsluttet,
                    opprettet: true,
                    saksnummer: Some(saksnummer.clone()),
                },
            )
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        Ok(ExecutionStepResult::completed())
    }

    fn observe_step_result(&self, _result: &ExecutionStepResult) {}

    async fn guard_sak_ikke_opprettet(
        &self,
        sak_id: Uuid,
    ) -> Result<ExecutionGuard<Option<SakState>>, EksekveringFeil> {
        let state = self
            .state_repo
            .hent_sak_state(sak_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        if let Some(existing) = &state {
            if existing.opprettet {
                return Ok(ExecutionGuard::skip("Sak finnes allerede i state"));
            }
        }

        Ok(ExecutionGuard::proceed(state))
    }

    async fn guard_journalpost_sak(
        &self,
        journalpost_id: Uuid,
        sak_id: Uuid,
    ) -> Result<ExecutionGuard<SakState>, EksekveringFeil> {
        let sak_state = self
            .state_repo
            .hent_sak_state(sak_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        let sak_state =
            sak_state.ok_or_else(|| EksekveringFeil::blocked("Sak finnes ikke i skuffen-state"))?;

        if sak_state.status == SakStatus::Avsluttet {
            return Err(EksekveringFeil::irrecoverable(
                "Kan ikke opprette journalpost på avsluttet sak",
            ));
        }

        let journalpost_state = self
            .state_repo
            .hent_journalpost_state(journalpost_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        if journalpost_state
            .as_ref()
            .and_then(|state| state.journalpostnummer)
            .is_some()
        {
            return Ok(ExecutionGuard::skip("Journalpost finnes allerede i state"));
        }

        Ok(ExecutionGuard::proceed(sak_state))
    }

    async fn guard_journalpost_finnes(
        &self,
        journalpost_id: Uuid,
    ) -> Result<ExecutionGuard<JournalpostState>, EksekveringFeil> {
        let journalpost_state = self
            .state_repo
            .hent_journalpost_state(journalpost_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        let Some(state) = journalpost_state else {
            return Err(EksekveringFeil::blocked(
                "Kan ikke legge til dokument før journalpost finnes",
            ));
        };

        Ok(ExecutionGuard::proceed(state))
    }

    async fn guard_dokument_ikke_lagt_til(
        &self,
        dokument_id: Uuid,
    ) -> Result<ExecutionGuard<Option<DokumentState>>, EksekveringFeil> {
        let dokument_state = self
            .state_repo
            .hent_dokument_state(dokument_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;

        if let Some(existing) = &dokument_state {
            if existing.lagt_til {
                return Ok(ExecutionGuard::skip("Dokument er allerede lagt til"));
            }
        }

        Ok(ExecutionGuard::proceed(dokument_state))
    }

    async fn guard_journalpost_kan_journalfores(
        &self,
        journalpost_id: Uuid,
    ) -> Result<ExecutionGuard<JournalpostState>, EksekveringFeil> {
        let journalpost_state = self
            .state_repo
            .hent_journalpost_state(journalpost_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        let Some(state) = journalpost_state else {
            return Err(EksekveringFeil::blocked("Journalpost finnes ikke i state"));
        };

        if state.har_feilede_dokumenter {
            return Err(EksekveringFeil::blocked(
                "Journalpost har feilede dokumenter",
            ));
        }

        Ok(ExecutionGuard::proceed(state))
    }

    async fn guard_journalpost_kan_avskrives(
        &self,
        journalpost_id: Uuid,
    ) -> Result<ExecutionGuard<JournalpostState>, EksekveringFeil> {
        let journalpost_state = self
            .state_repo
            .hent_journalpost_state(journalpost_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        let Some(state) = journalpost_state else {
            return Err(EksekveringFeil::blocked("Journalpost finnes ikke i state"));
        };

        if !state.journalfoert {
            return Err(EksekveringFeil::blocked("Journalpost er ikke journalført"));
        }

        Ok(ExecutionGuard::proceed(state))
    }

    async fn guard_sak_kan_avsluttes(
        &self,
        sak_id: Uuid,
    ) -> Result<ExecutionGuard<SakState>, EksekveringFeil> {
        let sak_state = self
            .state_repo
            .hent_sak_state(sak_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        let Some(state) = sak_state else {
            return Err(EksekveringFeil::blocked("Sak finnes ikke"));
        };

        if state.status == SakStatus::Avsluttet {
            return Ok(ExecutionGuard::skip("Sak er allerede avsluttet"));
        }

        let journalposter = self
            .state_repo
            .hent_journalposter_for_sak(sak_id)
            .await
            .map_err(|err| EksekveringFeil::recoverable(err.to_string()))?;
        for journalpost in journalposter {
            if journalpost.har_feilede_dokumenter {
                return Err(EksekveringFeil::blocked(
                    "Sak kan ikke avsluttes med feilede dokumenter",
                ));
            }

            match journalpost.journalposttype {
                'I' => {
                    if !journalpost.journalfoert || !journalpost.avskrevet {
                        return Err(EksekveringFeil::blocked(
                            "Inngående journalpost er ikke komplett",
                        ));
                    }
                }
                'U' => {
                    if !journalpost.journalfoert {
                        return Err(EksekveringFeil::blocked(
                            "Utgående journalpost er ikke komplett",
                        ));
                    }
                }
                'X' => {
                    if !journalpost.journalfoert {
                        return Err(EksekveringFeil::blocked("Internt notat er ikke komplett"));
                    }
                }
                _ => {
                    return Err(EksekveringFeil::blocked("Ukjent journalposttype i state"));
                }
            }
        }

        Ok(ExecutionGuard::proceed(state))
    }

    async fn hent_saksnummer(&self, sak_key: SakKey) -> Option<String> {
        match sak_key {
            SakKey::ClientReference(sak_id) => self
                .state_repo
                .hent_sak_state(sak_id)
                .await
                .ok()
                .flatten()
                .and_then(|state| state.saksnummer),
            SakKey::ArkivId(saksnummer) => Some(saksnummer.as_str().to_string()),
        }
    }

    async fn hent_journalpostnummer(&self, journalpost_id: Uuid) -> Option<i32> {
        self.state_repo
            .hent_journalpost_state(journalpost_id)
            .await
            .ok()
            .flatten()
            .and_then(|state| state.journalpostnummer)
    }

    async fn avslutt_med_feil(
        &self,
        envelope: &CommandEnvelope<Command>,
        err: EksekveringFeil,
    ) -> Result<ExecutionOutcome, anyhow::Error> {
        let status = match err.feiltype {
            EksekveringFeiltype::Recoverable => CommandStatus::Retrying,
            EksekveringFeiltype::Irrecoverable => CommandStatus::Error,
            EksekveringFeiltype::Blocked => CommandStatus::Blocked,
        };

        let status_for_event = status.clone();
        let is_terminal = matches!(
            &status_for_event,
            CommandStatus::Error | CommandStatus::Blocked
        );
        let refs_message = self.build_reference_message(envelope).await;
        let merged_message = match refs_message {
            Some(refs) if !err.melding.is_empty() => Some(format!("{} | {}", err.melding, refs)),
            Some(refs) => Some(refs),
            None => Some(err.melding.clone()),
        };
        let status_event_value =
            status_event(envelope, status_for_event, merged_message.clone(), None);
        self.status_publisher
            .publiser_status(status_event_value)
            .await?;

        if is_terminal {
            let (subject, _) = domain::eksekvering::typer::done_subject(envelope);
            self.done_publisher
                .publiser_done(&subject, envelope)
                .await?;
        }

        let outcome = match err.feiltype {
            EksekveringFeiltype::Recoverable => ExecutionOutcome::Retrying {
                last_error: merged_message,
            },
            EksekveringFeiltype::Irrecoverable => ExecutionOutcome::Error {
                last_error: merged_message,
            },
            EksekveringFeiltype::Blocked => ExecutionOutcome::Blocked {
                last_error: merged_message,
            },
        };

        Ok(outcome)
    }

    fn map_arkiv_feil(&self, err: anyhow::Error) -> EksekveringFeil {
        let original = err.to_string();
        let message = original
            .replace("sikri_recoverability=irrecoverable", "")
            .replace("sikri_recoverability=recoverable", "")
            .trim()
            .to_string();

        if original.contains("sikri_recoverability=irrecoverable") {
            return EksekveringFeil::irrecoverable(message);
        }
        EksekveringFeil::recoverable(message)
    }

    async fn resolve_arkiv_id_from_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Option<String> {
        let skuffen_id = self
            .id_mapping
            .get_skuffen_id(client_reference)
            .await
            .ok()??;
        self.id_mapping.get_arkiv_id(skuffen_id).await.ok()?
    }

    async fn build_reference_message(&self, envelope: &CommandEnvelope<Command>) -> Option<String> {
        let mut message = String::new();
        match &envelope.payload {
            Command::OpprettSak(cmd) => {
                if let Some(saksnummer) = self
                    .resolve_arkiv_id_from_client_reference(cmd.client_reference)
                    .await
                {
                    let _ = write!(message, "saksnummer={saksnummer}");
                }
            }
            Command::OpprettInngåendeJournalpost(cmd) => {
                message = self.build_journalpost_reference_message(&cmd.felles).await;
            }
            Command::OpprettUtgåendeJournalpost(cmd) => {
                message = self.build_journalpost_reference_message(&cmd.felles).await;
            }
            Command::OpprettInterntNotatJournalpost(cmd) => {
                message = self.build_journalpost_reference_message(&cmd.felles).await;
            }
            Command::AvsluttSak(cmd) => {
                let saksnummer = match &cmd.sak_key {
                    SakKey::ArkivId(saksnummer) => Some(saksnummer.as_str().to_string()),
                    SakKey::ClientReference(client_ref) => {
                        self.resolve_arkiv_id_from_client_reference(*client_ref)
                            .await
                    }
                };
                if let Some(saksnummer) = saksnummer {
                    let _ = write!(message, "saksnummer={saksnummer}");
                }
            }
        }

        if message.is_empty() {
            None
        } else {
            Some(message)
        }
    }

    async fn build_journalpost_reference_message(
        &self,
        felles: &lib_schemas::skuffen::command::journalpost::JournalpostCommon,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();

        let saksnummer = match &felles.sak_key {
            SakKey::ArkivId(saksnummer) => Some(saksnummer.as_str().to_string()),
            SakKey::ClientReference(client_ref) => {
                self.resolve_arkiv_id_from_client_reference(*client_ref)
                    .await
            }
        };
        if let Some(saksnummer) = saksnummer {
            parts.push(format!("saksnummer={saksnummer}"));
        }

        if let Some(journalpost_id) = self
            .resolve_arkiv_id_from_client_reference(felles.client_reference)
            .await
        {
            parts.push(format!("journalpostId={journalpost_id}"));
        }

        let mut dokument_ids: Vec<String> = Vec::new();
        for dokument in &felles.dokumenter {
            if let Some(dokument_id) = self
                .resolve_arkiv_id_from_client_reference(dokument.client_reference)
                .await
            {
                dokument_ids.push(dokument_id);
            }
        }
        if !dokument_ids.is_empty() {
            parts.push(format!("dokumentIds={}", dokument_ids.join(",")));
        }

        parts.join(" ")
    }
}
