use async_trait::async_trait;
use domain::eksekvering::plan::JournalpostType;
use domain::eksekvering::typer::{CommandLifecycleContext, CommandLifecycleEvent};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::dokument::Dokument;
use lib_schemas::skuffen::query::queries::SakKey;
use lib_schemas::skuffen::sak::Saksnummer;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::command::ports::eksekvering_port::{
    ArkivGateway, EksekveringKvitteringPublisher, EksekveringStatusPublisher,
    OpprettJournalpostResultat, Utsendingsvalg,
};
use crate::command::ports::execution_snapshot_port::{
    DokumentState, EksekveringSnapshotRepository, JournalpostOpprettetTransition,
    JournalpostOvergangVedJournalfoering, JournalpostState, SakState, SakTransition,
};
use crate::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::ports::ventende_kommando_wakeup_port::VentendeKommandoWakeup;
use crate::command::services::eksekver_kommando::{EksekverKommandoService, ExecutionOutcome};
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use lib_schemas::skuffen::status::SkuffenStatusErrorCode;

#[derive(Default)]
struct FakeExecutionData {
    saker: HashMap<Uuid, SakState>,
    journalposter: HashMap<Uuid, JournalpostState>,
    journalposter_per_sak: HashMap<Uuid, Vec<Uuid>>,
    dokumenter: HashMap<Uuid, DokumentState>,
}

#[derive(Clone, Default)]
struct FakeSnapshotRepository {
    data: Arc<Mutex<FakeExecutionData>>,
}

#[async_trait]
impl EksekveringSnapshotRepository for FakeSnapshotRepository {
    async fn hent_sak_state(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<SakState>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .saker
            .get(&Uuid::from(sak_id))
            .cloned())
    }

    async fn ensure_sak_state(
        &self,
        sak_id: SkuffenSakId,
        state: SakState,
    ) -> Result<SakState, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        Ok(data
            .saker
            .entry(Uuid::from(sak_id))
            .or_insert(state)
            .clone())
    }

    async fn anvend_sak_transition(
        &self,
        sak_id: SkuffenSakId,
        transition: SakTransition,
    ) -> Result<SakState, anyhow::Error> {
        let state = SakState {
            status: transition.status,
            opprettet: transition.opprettet,
            saksnummer: transition.saksnummer,
        };
        self.data
            .lock()
            .unwrap()
            .saker
            .insert(Uuid::from(sak_id), state.clone());
        Ok(state)
    }

    async fn hent_journalpost_state(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<JournalpostState>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .journalposter
            .get(&Uuid::from(journalpost_id))
            .cloned())
    }

    async fn ensure_journalpost_state(
        &self,
        journalpost_id: SkuffenJournalpostId,
        sak_id: SkuffenSakId,
        state: JournalpostState,
    ) -> Result<JournalpostState, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let journalpost_id = Uuid::from(journalpost_id);
        let sak_id = Uuid::from(sak_id);
        if let Some(existing) = data.journalposter.get(&journalpost_id).cloned() {
            return Ok(existing);
        }
        data.journalposter.insert(journalpost_id, state.clone());
        data.journalposter_per_sak
            .entry(sak_id)
            .or_default()
            .push(journalpost_id);
        Ok(state)
    }

    async fn anvend_journalpost_opprettet(
        &self,
        journalpost_id: SkuffenJournalpostId,
        transition: JournalpostOpprettetTransition,
    ) -> Result<JournalpostState, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let state = data
            .journalposter
            .entry(Uuid::from(journalpost_id))
            .or_insert(JournalpostState {
                journalfoert: false,
                avskrevet: false,
                ekspedert: false,
                har_feilede_dokumenter: false,
                med_utsending: false,
                journalposttype: JournalpostType::InterntNotat,
                journalpostnummer: None,
            });
        state.journalpostnummer = Some(transition.journalpostnummer);
        Ok(state.clone())
    }

    async fn anvend_journalpost_overgang_ved_journalfoering(
        &self,
        journalpost_id: SkuffenJournalpostId,
        transition: JournalpostOvergangVedJournalfoering,
    ) -> Result<JournalpostState, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let state = data
            .journalposter
            .get_mut(&Uuid::from(journalpost_id))
            .ok_or_else(|| anyhow::anyhow!("missing journalpost state"))?;
        state.journalfoert = transition.journalfoert;
        state.ekspedert = transition.ekspedert;
        Ok(state.clone())
    }

    async fn anvend_journalpost_avskrevet(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<JournalpostState, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let state = data
            .journalposter
            .get_mut(&Uuid::from(journalpost_id))
            .ok_or_else(|| anyhow::anyhow!("missing journalpost state"))?;
        state.avskrevet = true;
        Ok(state.clone())
    }

    async fn hent_journalposter_for_sak(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Vec<JournalpostState>, anyhow::Error> {
        let data = self.data.lock().unwrap();
        Ok(data
            .journalposter_per_sak
            .get(&Uuid::from(sak_id))
            .into_iter()
            .flatten()
            .filter_map(|journalpost_id| data.journalposter.get(journalpost_id).cloned())
            .collect())
    }

    async fn hent_dokument_state(
        &self,
        dokument_id: SkuffenDokumentId,
    ) -> Result<Option<DokumentState>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .dokumenter
            .get(&Uuid::from(dokument_id))
            .cloned())
    }

    async fn ensure_dokument_state(
        &self,
        dokument_id: SkuffenDokumentId,
        _journalpost_id: SkuffenJournalpostId,
        state: DokumentState,
    ) -> Result<DokumentState, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let dokument_id = Uuid::from(dokument_id);
        if let Some(existing) = data.dokumenter.get(&dokument_id).cloned() {
            return Ok(existing);
        }
        data.dokumenter.insert(dokument_id, state.clone());
        Ok(state)
    }

    async fn anvend_dokument_lagt_til(
        &self,
        dokument_id: SkuffenDokumentId,
        _journalpost_id: SkuffenJournalpostId,
    ) -> Result<DokumentState, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let state = data
            .dokumenter
            .entry(Uuid::from(dokument_id))
            .or_insert(DokumentState {
                lagt_til: false,
                irrecoverable_feil: false,
            });
        state.lagt_til = true;
        Ok(state.clone())
    }

    async fn anvend_dokument_irrecoverable_feil(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<DokumentState, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let dokument_state = {
            let state = data
                .dokumenter
                .entry(Uuid::from(dokument_id))
                .or_insert(DokumentState {
                    lagt_til: false,
                    irrecoverable_feil: false,
                });
            state.irrecoverable_feil = true;
            state.lagt_til = false;
            state.clone()
        };
        let journalpost_state = data
            .journalposter
            .get_mut(&Uuid::from(journalpost_id))
            .ok_or_else(|| anyhow::anyhow!("missing journalpost state"))?;
        journalpost_state.har_feilede_dokumenter = true;
        Ok(dokument_state)
    }
}

#[derive(Default)]
struct FakeIdMappingData {
    client_to_skuffen: HashMap<Uuid, Uuid>,
    skuffen_to_arkiv: HashMap<Uuid, String>,
}

#[derive(Clone, Default)]
struct FakeIdMappingRepository {
    data: Arc<Mutex<FakeIdMappingData>>,
}

#[async_trait]
impl IdMappingRepository for FakeIdMappingRepository {
    async fn has_processed_command(&self, _command_id: Uuid) -> Result<bool, anyhow::Error> {
        Ok(false)
    }

    async fn register_mapping(
        &self,
        _command_id: Uuid,
        _client_reference: Uuid,
        _skuffen_id: SkuffenSakId,
        _command: &Command,
        _arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn register_document_mapping(
        &self,
        _command_id: Uuid,
        _client_reference: Uuid,
        _skuffen_id: SkuffenDokumentId,
        _arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn oppdater_arkiv_id_for_client_reference(
        &self,
        client_reference: Uuid,
        arkiv_id: String,
    ) -> Result<(), anyhow::Error> {
        let Some(skuffen_id) = self
            .data
            .lock()
            .unwrap()
            .client_to_skuffen
            .get(&client_reference)
            .copied()
        else {
            return Err(anyhow::anyhow!("missing mapping"));
        };
        self.data
            .lock()
            .unwrap()
            .skuffen_to_arkiv
            .insert(skuffen_id, arkiv_id);
        Ok(())
    }

    async fn hent_arkiv_id_fra_mapping(
        &self,
        skuffen_id: SkuffenSakId,
    ) -> Result<Option<String>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .skuffen_to_arkiv
            .get(&Uuid::from(skuffen_id))
            .cloned())
    }

    async fn hent_sak_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .client_to_skuffen
            .get(&client_reference)
            .copied()
            .map(SkuffenSakId::from))
    }

    async fn hent_journalpost_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenJournalpostId>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .client_to_skuffen
            .get(&client_reference)
            .copied()
            .map(SkuffenJournalpostId::from))
    }

    async fn hent_dokument_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenDokumentId>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .client_to_skuffen
            .get(&client_reference)
            .copied()
            .map(SkuffenDokumentId::from))
    }

    async fn hent_sak_id_fra_arkiv_id_i_mapping(
        &self,
        arkiv_id: &str,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error> {
        Ok(self.data.lock().unwrap().skuffen_to_arkiv.iter().find_map(
            |(skuffen_id, stored_arkiv_id)| {
                (stored_arkiv_id == arkiv_id).then_some(SkuffenSakId::from(*skuffen_id))
            },
        ))
    }

    async fn hent_eller_opprett_skuffen_id_for_arkiv_id(
        &self,
        _entity_type: MappingEntityType,
        _arkiv_id: &str,
    ) -> Result<SkuffenSakId, anyhow::Error> {
        Err(anyhow::anyhow!(
            "should not create mapping during execution"
        ))
    }

    async fn delete_arkiv_mapping(
        &self,
        _entity_type: MappingEntityType,
        _arkiv_id: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[derive(Default)]
struct FakeArkivData {
    opprett_sak_calls: usize,
    opprett_journalpost_calls: usize,
    legg_til_vedlegg_calls: usize,
    sett_status_calls: Vec<(i32, String)>,
    avskriv_calls: Vec<i32>,
    avslutt_sak_calls: Vec<String>,
    fail_legg_til_vedlegg: Option<String>,
    fail_sett_status: Option<String>,
}

#[derive(Clone, Default)]
struct FakeArkivGateway {
    data: Arc<Mutex<FakeArkivData>>,
}

#[async_trait]
impl ArkivGateway for FakeArkivGateway {
    async fn opprett_sak(
        &self,
        _command: &CommandEnvelope<Command>,
    ) -> Result<String, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        data.opprett_sak_calls += 1;
        Ok("2026/900001".to_string())
    }

    async fn opprett_journalpost(
        &self,
        _command: &CommandEnvelope<Command>,
        _saksnummer: &str,
        _utsending: Option<Utsendingsvalg>,
    ) -> Result<OpprettJournalpostResultat, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        data.opprett_journalpost_calls += 1;
        Ok(OpprettJournalpostResultat { journalpost_id: 42 })
    }

    async fn legg_til_vedlegg(
        &self,
        _command: &CommandEnvelope<Command>,
        _journalpost_id: i32,
        dokument_ids: Vec<Uuid>,
    ) -> Result<Vec<Option<i32>>, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        data.legg_til_vedlegg_calls += 1;
        if let Some(feilmelding) = data.fail_legg_til_vedlegg.clone() {
            return Err(anyhow::anyhow!(feilmelding));
        }
        Ok(dokument_ids.into_iter().map(|_| Some(70001)).collect())
    }

    async fn sett_journalpost_status(
        &self,
        journalpost_id: i32,
        status: &str,
    ) -> Result<(), anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        data.sett_status_calls
            .push((journalpost_id, status.to_string()));
        if let Some(feilmelding) = data.fail_sett_status.clone() {
            return Err(anyhow::anyhow!(feilmelding));
        }
        Ok(())
    }

    async fn avskriv_journalpost(
        &self,
        journalpost_id: i32,
        _avskrivingsmaate: &str,
    ) -> Result<(), anyhow::Error> {
        self.data.lock().unwrap().avskriv_calls.push(journalpost_id);
        Ok(())
    }

    async fn avslutt_sak(&self, saksnummer: &str) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .avslutt_sak_calls
            .push(saksnummer.to_string());
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeStatusPublisher {
    events: Arc<Mutex<Vec<CommandLifecycleEvent>>>,
}

#[async_trait]
impl EksekveringStatusPublisher for FakeStatusPublisher {
    async fn publiser_status(&self, event: CommandLifecycleEvent) -> Result<(), anyhow::Error> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeDonePublisher {
    calls: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl EksekveringKvitteringPublisher for FakeDonePublisher {
    async fn publiser_done(
        &self,
        subject: &str,
        _command: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error> {
        self.calls.lock().unwrap().push(subject.to_string());
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeStatusContextResolver;

#[async_trait]
impl CommandOutwardStatusProjector for FakeStatusContextResolver {
    async fn resolve_context(
        &self,
        _envelope: &CommandEnvelope<Command>,
    ) -> Result<CommandLifecycleContext, anyhow::Error> {
        Ok(CommandLifecycleContext::default())
    }
}

#[derive(Clone, Default)]
struct RecordingWakeupService {
    sak_calls: Arc<Mutex<Vec<Uuid>>>,
    journalpost_calls: Arc<Mutex<Vec<Uuid>>>,
}

#[async_trait]
impl VentendeKommandoWakeup for RecordingWakeupService {
    async fn etter_sak_endret(&self, sak_id: SkuffenSakId) -> Result<(), anyhow::Error> {
        self.sak_calls.lock().unwrap().push(Uuid::from(sak_id));
        Ok(())
    }

    async fn etter_journalpost_endret(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<(), anyhow::Error> {
        self.journalpost_calls
            .lock()
            .unwrap()
            .push(Uuid::from(journalpost_id));
        Ok(())
    }
}

fn build_service(
    snapshot_repo: FakeSnapshotRepository,
    arkiv_gateway: FakeArkivGateway,
    status_publisher: FakeStatusPublisher,
    done_publisher: FakeDonePublisher,
    id_mapping: FakeIdMappingRepository,
) -> EksekverKommandoService {
    build_service_with_wakeup(
        snapshot_repo,
        arkiv_gateway,
        status_publisher,
        done_publisher,
        id_mapping,
        RecordingWakeupService::default(),
    )
}

fn build_service_with_wakeup(
    snapshot_repo: FakeSnapshotRepository,
    arkiv_gateway: FakeArkivGateway,
    status_publisher: FakeStatusPublisher,
    done_publisher: FakeDonePublisher,
    id_mapping: FakeIdMappingRepository,
    wakeup_service: RecordingWakeupService,
) -> EksekverKommandoService {
    EksekverKommandoService::new(
        Box::new(snapshot_repo),
        Box::new(arkiv_gateway),
        Box::new(status_publisher),
        Box::new(done_publisher),
        Box::new(id_mapping),
        Box::new(FakeStatusContextResolver),
        Box::new(wakeup_service),
    )
}

#[tokio::test]
async fn handle_returns_blocked_when_journalpost_waits_for_sak() {
    let state_repo = FakeSnapshotRepository::default();
    let arkiv_gateway = FakeArkivGateway::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();
    let id_mapping = FakeIdMappingRepository::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    let dokument_client_reference = Uuid::new_v4();
    let dokument_skuffen_id = Uuid::new_v4();
    {
        let mut data = id_mapping.data.lock().unwrap();
        data.client_to_skuffen
            .insert(sak_client_reference, sak_skuffen_id);
        data.client_to_skuffen
            .insert(journalpost_client_reference, journalpost_skuffen_id);
        data.client_to_skuffen
            .insert(dokument_client_reference, dokument_skuffen_id);
    }

    let service = build_service(
        state_repo,
        arkiv_gateway.clone(),
        status_publisher.clone(),
        done_publisher.clone(),
        id_mapping,
    );

    let outcome = service
        .handle(
            make_internt_notat_command(
                journalpost_client_reference,
                sak_client_reference,
                dokument_client_reference,
            ),
            1,
        )
        .await
        .unwrap();

    assert!(matches!(
        outcome,
        ExecutionOutcome::Blocked {
            last_error: Some(ref detail),
            ..
        } if detail == "Sak finnes ikke i skuffen-state"
    ));
    assert_eq!(
        arkiv_gateway.data.lock().unwrap().opprett_journalpost_calls,
        0
    );
    assert!(done_publisher.calls.lock().unwrap().is_empty());
    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].detail.as_deref(),
        Some("Sak finnes ikke i skuffen-state")
    );
    assert_eq!(
        events[0].error_code,
        Some(SkuffenStatusErrorCode::PrerequisitePending)
    );
}

#[tokio::test]
async fn journalfoering_skips_when_already_journalfoert() {
    let state_repo = FakeSnapshotRepository::default();
    let arkiv_gateway = FakeArkivGateway::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();
    let id_mapping = FakeIdMappingRepository::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    let hoveddokument_client_reference = Uuid::new_v4();
    let hoveddokument_skuffen_id = Uuid::new_v4();
    let vedlegg_client_reference = Uuid::new_v4();
    let vedlegg_skuffen_id = Uuid::new_v4();
    {
        let mut mapping_data = id_mapping.data.lock().unwrap();
        mapping_data
            .client_to_skuffen
            .insert(sak_client_reference, sak_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(journalpost_client_reference, journalpost_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(hoveddokument_client_reference, hoveddokument_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(vedlegg_client_reference, vedlegg_skuffen_id);
    }
    {
        let mut execution_data = state_repo.data.lock().unwrap();
        execution_data.saker.insert(
            sak_skuffen_id,
            SakState {
                status: crate::command::ports::execution_snapshot_port::SakStatus::UnderBehandling,
                opprettet: true,
                saksnummer: Some("2026/1".to_string()),
            },
        );
        execution_data.journalposter.insert(
            journalpost_skuffen_id,
            JournalpostState {
                journalfoert: true,
                avskrevet: false,
                ekspedert: false,
                har_feilede_dokumenter: false,
                med_utsending: false,
                journalposttype: JournalpostType::InterntNotat,
                journalpostnummer: Some(42),
            },
        );
    }

    let service = build_service(
        state_repo,
        arkiv_gateway.clone(),
        status_publisher,
        done_publisher,
        id_mapping,
    );

    let outcome = service
        .handle(
            make_internt_notat_command_with_documents(
                journalpost_client_reference,
                sak_client_reference,
                vec![hoveddokument_client_reference, vedlegg_client_reference],
            ),
            1,
        )
        .await
        .unwrap();

    assert_eq!(outcome, ExecutionOutcome::Ok);
    assert!(arkiv_gateway
        .data
        .lock()
        .unwrap()
        .sett_status_calls
        .is_empty());
}

#[tokio::test]
async fn avskriving_skips_when_already_avskrevet() {
    let state_repo = FakeSnapshotRepository::default();
    let arkiv_gateway = FakeArkivGateway::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();
    let id_mapping = FakeIdMappingRepository::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    let dokument_client_reference = Uuid::new_v4();
    let dokument_skuffen_id = Uuid::new_v4();
    {
        let mut mapping_data = id_mapping.data.lock().unwrap();
        mapping_data
            .client_to_skuffen
            .insert(sak_client_reference, sak_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(journalpost_client_reference, journalpost_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(dokument_client_reference, dokument_skuffen_id);
    }
    {
        let mut execution_data = state_repo.data.lock().unwrap();
        execution_data.saker.insert(
            sak_skuffen_id,
            SakState {
                status: crate::command::ports::execution_snapshot_port::SakStatus::UnderBehandling,
                opprettet: true,
                saksnummer: Some("2026/1".to_string()),
            },
        );
        execution_data.journalposter.insert(
            journalpost_skuffen_id,
            JournalpostState {
                journalfoert: true,
                avskrevet: true,
                ekspedert: false,
                har_feilede_dokumenter: false,
                med_utsending: false,
                journalposttype: JournalpostType::Inngaende,
                journalpostnummer: Some(42),
            },
        );
    }

    let service = build_service(
        state_repo,
        arkiv_gateway.clone(),
        status_publisher,
        done_publisher,
        id_mapping,
    );

    let outcome = service
        .handle(
            make_avskriv_command(
                journalpost_client_reference,
                sak_client_reference,
                dokument_client_reference,
            ),
            1,
        )
        .await
        .unwrap();

    assert_eq!(outcome, ExecutionOutcome::Ok);
    assert!(arkiv_gateway.data.lock().unwrap().avskriv_calls.is_empty());
}

#[tokio::test]
async fn handle_returns_retrying_for_gateway_failure() {
    let state_repo = FakeSnapshotRepository::default();
    let arkiv_gateway = FakeArkivGateway::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();
    let id_mapping = FakeIdMappingRepository::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    let dokument_client_reference = Uuid::new_v4();
    let dokument_skuffen_id = Uuid::new_v4();
    {
        let mut mapping_data = id_mapping.data.lock().unwrap();
        mapping_data
            .client_to_skuffen
            .insert(sak_client_reference, sak_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(journalpost_client_reference, journalpost_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(dokument_client_reference, dokument_skuffen_id);
    }
    {
        let mut execution_data = state_repo.data.lock().unwrap();
        execution_data.saker.insert(
            sak_skuffen_id,
            SakState {
                status: crate::command::ports::execution_snapshot_port::SakStatus::UnderBehandling,
                opprettet: true,
                saksnummer: Some("2026/1".to_string()),
            },
        );
        execution_data.journalposter.insert(
            journalpost_skuffen_id,
            JournalpostState {
                journalfoert: false,
                avskrevet: false,
                ekspedert: false,
                har_feilede_dokumenter: false,
                med_utsending: false,
                journalposttype: JournalpostType::InterntNotat,
                journalpostnummer: Some(42),
            },
        );
    }
    arkiv_gateway.data.lock().unwrap().fail_sett_status = Some("timeout".to_string());

    let service = build_service(
        state_repo,
        arkiv_gateway,
        status_publisher.clone(),
        done_publisher,
        id_mapping,
    );

    let outcome = service
        .handle(
            make_internt_notat_command(
                journalpost_client_reference,
                sak_client_reference,
                dokument_client_reference,
            ),
            2,
        )
        .await
        .unwrap();

    assert!(matches!(
        outcome,
        ExecutionOutcome::Retrying {
            last_error: Some(ref detail)
        } if detail == "timeout"
    ));
    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].error_code,
        Some(SkuffenStatusErrorCode::TemporaryUnavailable)
    );
}

#[tokio::test]
async fn irrecoverable_dokumentfeil_markerer_dokument_state_og_feiler_kommandoen() {
    let state_repo = FakeSnapshotRepository::default();
    let arkiv_gateway = FakeArkivGateway::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();
    let id_mapping = FakeIdMappingRepository::default();
    let wakeup_service = RecordingWakeupService::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    let hoveddokument_client_reference = Uuid::new_v4();
    let hoveddokument_skuffen_id = Uuid::new_v4();
    let vedlegg_client_reference = Uuid::new_v4();
    let vedlegg_skuffen_id = Uuid::new_v4();
    {
        let mut mapping_data = id_mapping.data.lock().unwrap();
        mapping_data
            .client_to_skuffen
            .insert(sak_client_reference, sak_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(journalpost_client_reference, journalpost_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(hoveddokument_client_reference, hoveddokument_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(vedlegg_client_reference, vedlegg_skuffen_id);
    }
    {
        let mut execution_data = state_repo.data.lock().unwrap();
        execution_data.saker.insert(
            sak_skuffen_id,
            SakState {
                status: crate::command::ports::execution_snapshot_port::SakStatus::UnderBehandling,
                opprettet: true,
                saksnummer: Some("2026/1".to_string()),
            },
        );
        execution_data.journalposter.insert(
            journalpost_skuffen_id,
            JournalpostState {
                journalfoert: false,
                avskrevet: false,
                ekspedert: false,
                har_feilede_dokumenter: false,
                med_utsending: false,
                journalposttype: JournalpostType::InterntNotat,
                journalpostnummer: Some(42),
            },
        );
    }
    arkiv_gateway.data.lock().unwrap().fail_legg_til_vedlegg =
        Some("sikri_recoverability=irrecoverable dokument avvist".to_string());

    let service = build_service_with_wakeup(
        state_repo.clone(),
        arkiv_gateway,
        status_publisher.clone(),
        done_publisher,
        id_mapping,
        wakeup_service.clone(),
    );

    let outcome = service
        .handle(
            make_internt_notat_command_with_documents(
                journalpost_client_reference,
                sak_client_reference,
                vec![hoveddokument_client_reference, vedlegg_client_reference],
            ),
            1,
        )
        .await
        .unwrap();

    assert!(matches!(
        outcome,
        ExecutionOutcome::Error {
            last_error: Some(ref detail)
        } if detail.contains("dokument avvist")
    ));
    assert!(state_repo
        .data
        .lock()
        .unwrap()
        .dokumenter
        .get(&vedlegg_skuffen_id)
        .is_some_and(|state| state.irrecoverable_feil));
    assert!(state_repo
        .data
        .lock()
        .unwrap()
        .journalposter
        .get(&journalpost_skuffen_id)
        .is_some_and(|state| state.har_feilede_dokumenter));
    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].error_code,
        Some(SkuffenStatusErrorCode::ProcessingFailed)
    );
    assert!(!wakeup_service.journalpost_calls.lock().unwrap().is_empty());
    assert!(!wakeup_service.sak_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn already_completed_journalfoering_triggerer_fortsatt_wakeup() {
    let state_repo = FakeSnapshotRepository::default();
    let arkiv_gateway = FakeArkivGateway::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();
    let id_mapping = FakeIdMappingRepository::default();
    let wakeup_service = RecordingWakeupService::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    let hoveddokument_client_reference = Uuid::new_v4();
    let hoveddokument_skuffen_id = Uuid::new_v4();
    let vedlegg_client_reference = Uuid::new_v4();
    let vedlegg_skuffen_id = Uuid::new_v4();
    {
        let mut mapping_data = id_mapping.data.lock().unwrap();
        mapping_data
            .client_to_skuffen
            .insert(sak_client_reference, sak_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(journalpost_client_reference, journalpost_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(hoveddokument_client_reference, hoveddokument_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(vedlegg_client_reference, vedlegg_skuffen_id);
    }
    {
        let mut execution_data = state_repo.data.lock().unwrap();
        execution_data.saker.insert(
            sak_skuffen_id,
            SakState {
                status: crate::command::ports::execution_snapshot_port::SakStatus::UnderBehandling,
                opprettet: true,
                saksnummer: Some("2026/1".to_string()),
            },
        );
        execution_data.journalposter.insert(
            journalpost_skuffen_id,
            JournalpostState {
                journalfoert: true,
                avskrevet: false,
                ekspedert: false,
                har_feilede_dokumenter: false,
                med_utsending: false,
                journalposttype: JournalpostType::InterntNotat,
                journalpostnummer: Some(42),
            },
        );
    }

    let service = build_service_with_wakeup(
        state_repo,
        arkiv_gateway,
        status_publisher,
        done_publisher,
        id_mapping,
        wakeup_service.clone(),
    );

    let outcome = service
        .handle(
            make_internt_notat_command_with_documents(
                journalpost_client_reference,
                sak_client_reference,
                vec![hoveddokument_client_reference, vedlegg_client_reference],
            ),
            1,
        )
        .await
        .unwrap();

    assert_eq!(outcome, ExecutionOutcome::Ok);
    assert!(!wakeup_service.journalpost_calls.lock().unwrap().is_empty());
    assert!(!wakeup_service.sak_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn successful_opprett_sak_publishes_done() {
    let state_repo = FakeSnapshotRepository::default();
    let arkiv_gateway = FakeArkivGateway::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();
    let id_mapping = FakeIdMappingRepository::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    id_mapping
        .data
        .lock()
        .unwrap()
        .client_to_skuffen
        .insert(sak_client_reference, sak_skuffen_id);

    let service = build_service(
        state_repo.clone(),
        arkiv_gateway.clone(),
        status_publisher.clone(),
        done_publisher.clone(),
        id_mapping,
    );

    let outcome = service
        .handle(make_opprett_sak_command(sak_client_reference), 1)
        .await
        .unwrap();

    assert_eq!(outcome, ExecutionOutcome::Ok);
    assert_eq!(arkiv_gateway.data.lock().unwrap().opprett_sak_calls, 1);
    assert_eq!(done_publisher.calls.lock().unwrap().len(), 1);
    let saved_sak = state_repo
        .data
        .lock()
        .unwrap()
        .saker
        .get(&sak_skuffen_id)
        .cloned()
        .unwrap();
    assert_eq!(saved_sak.saksnummer.as_deref(), Some("2026/900001"));
    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail, None);
    assert_eq!(events[0].context.saksnummer.as_deref(), Some("2026/900001"));
}

#[tokio::test]
async fn successful_journalpost_execution_merges_report_ids_into_status_context() {
    let state_repo = FakeSnapshotRepository::default();
    let arkiv_gateway = FakeArkivGateway::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();
    let id_mapping = FakeIdMappingRepository::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    let hoveddokument_client_reference = Uuid::new_v4();
    let hoveddokument_skuffen_id = Uuid::new_v4();
    let vedlegg_client_reference = Uuid::new_v4();
    let vedlegg_skuffen_id = Uuid::new_v4();
    {
        let mut mapping_data = id_mapping.data.lock().unwrap();
        mapping_data
            .client_to_skuffen
            .insert(sak_client_reference, sak_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(journalpost_client_reference, journalpost_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(hoveddokument_client_reference, hoveddokument_skuffen_id);
        mapping_data
            .client_to_skuffen
            .insert(vedlegg_client_reference, vedlegg_skuffen_id);
    }
    {
        let mut execution_data = state_repo.data.lock().unwrap();
        execution_data.saker.insert(
            sak_skuffen_id,
            SakState {
                status: crate::command::ports::execution_snapshot_port::SakStatus::UnderBehandling,
                opprettet: true,
                saksnummer: Some("2026/1".to_string()),
            },
        );
    }

    let service = build_service(
        state_repo,
        arkiv_gateway,
        status_publisher.clone(),
        done_publisher,
        id_mapping,
    );

    let outcome = service
        .handle(
            make_internt_notat_command_with_documents(
                journalpost_client_reference,
                sak_client_reference,
                vec![hoveddokument_client_reference, vedlegg_client_reference],
            ),
            1,
        )
        .await
        .unwrap();

    assert_eq!(outcome, ExecutionOutcome::Ok);

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].context.saksnummer.as_deref(), Some("2026/1"));
    assert_eq!(events[0].context.journalpost_id.as_deref(), Some("42"));
    assert_eq!(events[0].context.dokument_ids, vec!["70001".to_string()]);
}

#[tokio::test]
async fn avslutt_sak_med_arkiv_id_bruker_mapping_og_avslutter_sak() {
    let state_repo = FakeSnapshotRepository::default();
    let arkiv_gateway = FakeArkivGateway::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();
    let id_mapping = FakeIdMappingRepository::default();

    let skuffen_sak_id = Uuid::new_v4();
    {
        let mut mapping_data = id_mapping.data.lock().unwrap();
        mapping_data
            .skuffen_to_arkiv
            .insert(skuffen_sak_id, "2025/456".to_string());
    }
    {
        let mut execution_data = state_repo.data.lock().unwrap();
        execution_data.saker.insert(
            skuffen_sak_id,
            SakState {
                status: crate::command::ports::execution_snapshot_port::SakStatus::UnderBehandling,
                opprettet: true,
                saksnummer: Some("2025/456".to_string()),
            },
        );
    }

    let service = build_service(
        state_repo.clone(),
        arkiv_gateway.clone(),
        status_publisher,
        done_publisher,
        id_mapping,
    );

    let outcome = service
        .handle(
            make_avslutt_sak_command(SakKey::ArkivId(Saksnummer::new("2025/456").unwrap())),
            1,
        )
        .await
        .unwrap();

    assert_eq!(outcome, ExecutionOutcome::Ok);
    assert_eq!(
        arkiv_gateway.data.lock().unwrap().avslutt_sak_calls,
        vec!["2025/456".to_string()]
    );
    assert_eq!(
        state_repo
            .data
            .lock()
            .unwrap()
            .saker
            .get(&skuffen_sak_id)
            .unwrap()
            .status,
        crate::command::ports::execution_snapshot_port::SakStatus::Avsluttet
    );
}

fn make_opprett_sak_command(sak_client_reference: Uuid) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel("Sak".to_string()),
            arkivdel: Arkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())
                .unwrap(),
            tilgang: None,
        }),
    }
}

fn make_internt_notat_command(
    journalpost_client_reference: Uuid,
    sak_client_reference: Uuid,
    dokument_client_reference: Uuid,
) -> CommandEnvelope<Command> {
    make_internt_notat_command_with_documents(
        journalpost_client_reference,
        sak_client_reference,
        vec![dokument_client_reference],
    )
}

fn make_internt_notat_command_with_documents(
    journalpost_client_reference: Uuid,
    sak_client_reference: Uuid,
    dokument_client_references: Vec<Uuid>,
) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
            felles: JournalpostCommon {
                client_reference: journalpost_client_reference,
                tittel: "Internt notat".to_string(),
                dokument_dato: "2025-01-01".to_string(),
                saksbehandler: "Z12345".to_string(),
                saksbehandler_enhet: "42".to_string(),
                tilgang: None,
                dokumenter: dokument_client_references
                    .into_iter()
                    .map(|client_reference| Dokument {
                        client_reference,
                        tittel: "Vedlegg".to_string(),
                        filtype: "PDF".to_string(),
                        dokument_referanse: Uuid::new_v4(),
                    })
                    .collect(),
                sak_key: SakKey::ClientReference(sak_client_reference),
                kildesystem: None,
            },
        }),
    }
}

fn make_avskriv_command(
    journalpost_client_reference: Uuid,
    sak_client_reference: Uuid,
    dokument_client_reference: Uuid,
) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettInngåendeJournalpost(
            lib_schemas::skuffen::command::journalpost::OpprettInngåendeJournalpost {
                felles: JournalpostCommon {
                    client_reference: journalpost_client_reference,
                    tittel: "Inngaaende".to_string(),
                    dokument_dato: "2025-01-01".to_string(),
                    saksbehandler: "Z12345".to_string(),
                    saksbehandler_enhet: "42".to_string(),
                    tilgang: None,
                    dokumenter: vec![Dokument {
                        client_reference: dokument_client_reference,
                        tittel: "Vedlegg".to_string(),
                        filtype: "PDF".to_string(),
                        dokument_referanse: Uuid::new_v4(),
                    }],
                    sak_key: SakKey::ClientReference(sak_client_reference),
                    kildesystem: None,
                },
                avsender: "Avsender".to_string(),
                mottaker: None,
            },
        ),
    }
}

fn make_avslutt_sak_command(sak_key: SakKey) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::AvsluttSak(AvsluttSak { sak_key }),
    }
}
