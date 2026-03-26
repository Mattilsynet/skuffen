use async_trait::async_trait;
use domain::eksekvering::execution::Ventegrunn;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::plan::JournalpostType;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::AvsluttSak;
use lib_schemas::skuffen::dokument::Dokument;
use lib_schemas::skuffen::query::queries::SakKey;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::command::ports::command_execution_port::{
    CommandExecutionRepository, EksekveringKommando, EksekveringsregistreringResultat,
    NyKommandoEksekvering,
};
use crate::command::ports::eksekvering_port::{
    EksekveringKvitteringPublisher, EksekveringStatusPublisher,
};
use crate::command::ports::execution_registration_port::EksekveringssystemRegistration;
use crate::command::ports::execution_snapshot_port::{
    DokumentState, EksekveringSnapshotRepository, JournalpostOpprettetTransition,
    JournalpostOvergangVedJournalfoering, JournalpostState, SakState, SakStatus, SakTransition,
};
use crate::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::services::reevaluer_ventende_kommandoer::ReevaluerVentendeKommandoerService;
use domain::eksekvering::typer::{CommandLifecycleContext, CommandLifecycleEvent};

#[derive(Default)]
struct FakeSnapshotData {
    saker: HashMap<Uuid, SakState>,
    journalposter: HashMap<Uuid, JournalpostState>,
    journalposter_per_sak: HashMap<Uuid, Vec<Uuid>>,
    dokumenter: HashMap<Uuid, DokumentState>,
}

#[derive(Clone, Default)]
struct FakeSnapshotRepository {
    data: Arc<Mutex<FakeSnapshotData>>,
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
        self.data
            .lock()
            .unwrap()
            .saker
            .entry(Uuid::from(sak_id))
            .or_insert(state.clone());
        Ok(state)
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
        data.journalposter
            .entry(journalpost_id)
            .or_insert(state.clone());
        data.journalposter_per_sak
            .entry(Uuid::from(sak_id))
            .or_default()
            .push(journalpost_id);
        Ok(state)
    }

    async fn anvend_journalpost_opprettet(
        &self,
        _journalpost_id: SkuffenJournalpostId,
        _transition: JournalpostOpprettetTransition,
    ) -> Result<JournalpostState, anyhow::Error> {
        unreachable!()
    }

    async fn anvend_journalpost_overgang_ved_journalfoering(
        &self,
        _journalpost_id: SkuffenJournalpostId,
        _transition: JournalpostOvergangVedJournalfoering,
    ) -> Result<JournalpostState, anyhow::Error> {
        unreachable!()
    }

    async fn anvend_journalpost_avskrevet(
        &self,
        _journalpost_id: SkuffenJournalpostId,
    ) -> Result<JournalpostState, anyhow::Error> {
        unreachable!()
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
        self.data
            .lock()
            .unwrap()
            .dokumenter
            .entry(Uuid::from(dokument_id))
            .or_insert(state.clone());
        Ok(state)
    }

    async fn anvend_dokument_lagt_til(
        &self,
        _dokument_id: SkuffenDokumentId,
        _journalpost_id: SkuffenJournalpostId,
    ) -> Result<DokumentState, anyhow::Error> {
        unreachable!()
    }

    async fn anvend_dokument_irrecoverable_feil(
        &self,
        _dokument_id: SkuffenDokumentId,
        _journalpost_id: SkuffenJournalpostId,
    ) -> Result<DokumentState, anyhow::Error> {
        unreachable!()
    }
}

#[derive(Default)]
struct FakeExecutionData {
    ventende_for_sak: HashMap<Uuid, Vec<EksekveringKommando>>,
    ventende_for_journalpost: HashMap<Uuid, Vec<EksekveringKommando>>,
    oppdatert_til_klar: Vec<Uuid>,
    oppdatert_til_venter: Vec<(Uuid, String, String)>,
    oppdatert_til_feil: Vec<(Uuid, String)>,
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
    subjects: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl EksekveringKvitteringPublisher for FakeDonePublisher {
    async fn publiser_done(
        &self,
        subject: &str,
        _command: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error> {
        self.subjects.lock().unwrap().push(subject.to_string());
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeStatusProjector;

#[async_trait]
impl CommandOutwardStatusProjector for FakeStatusProjector {
    async fn resolve_context(
        &self,
        _envelope: &CommandEnvelope<Command>,
    ) -> Result<CommandLifecycleContext, anyhow::Error> {
        Ok(CommandLifecycleContext::default())
    }
}

#[derive(Clone, Default)]
struct FakeExecutionRepository {
    data: Arc<Mutex<FakeExecutionData>>,
}

#[async_trait]
impl CommandExecutionRepository for FakeExecutionRepository {
    async fn try_acquire_executor_lock(&self, _executor_id: &str) -> Result<bool, anyhow::Error> {
        Ok(true)
    }
    async fn opprett(
        &self,
        _registration: &EksekveringssystemRegistration,
        _ny: NyKommandoEksekvering,
    ) -> Result<EksekveringsregistreringResultat, anyhow::Error> {
        unreachable!()
    }
    async fn marker_utfores_venter_publisert(
        &self,
        _command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn hent_neste_kjorbare(&self) -> Result<Option<EksekveringKommando>, anyhow::Error> {
        Ok(None)
    }
    async fn marker_kjorer(&self, _command_id: Uuid) -> Result<i32, anyhow::Error> {
        Ok(1)
    }
    async fn registrer_forsok(
        &self,
        _command_id: Uuid,
        _attempt_no: i32,
        _executor_id: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn marker_ok(&self, _command_id: Uuid, _attempt_no: i32) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn marker_retry_venter(
        &self,
        _command_id: Uuid,
        _attempt_no: i32,
        _detalj: &str,
        _retry_ready_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn marker_venter(
        &self,
        _command_id: Uuid,
        _attempt_no: i32,
        _grunn: &Ventegrunn,
        _detalj: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn marker_feil(
        &self,
        _command_id: Uuid,
        _attempt_no: i32,
        _detalj: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn marker_forsok_avbrutt(
        &self,
        _command_id: Uuid,
        _attempt_no: i32,
        _detalj: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn hent_ventende_for_sak(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .ventende_for_sak
            .get(&Uuid::from(sak_id))
            .cloned()
            .unwrap_or_default())
    }

    async fn hent_ventende_for_journalpost(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .ventende_for_journalpost
            .get(&Uuid::from(journalpost_id))
            .cloned()
            .unwrap_or_default())
    }

    async fn oppdater_til_klar(&self, command_id: Uuid) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .oppdatert_til_klar
            .push(command_id);
        Ok(())
    }

    async fn oppdater_venter(
        &self,
        command_id: Uuid,
        grunn: &Ventegrunn,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        self.data.lock().unwrap().oppdatert_til_venter.push((
            command_id,
            grunn.kind_code().to_string(),
            detalj.to_string(),
        ));
        Ok(())
    }

    async fn oppdater_til_feil(&self, command_id: Uuid, detalj: &str) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .oppdatert_til_feil
            .push((command_id, detalj.to_string()));
        Ok(())
    }

    async fn reset_kjorer_til_klar(&self) -> Result<u64, anyhow::Error> {
        Ok(0)
    }
}

#[derive(Default)]
struct FakeIdMappingData {
    client_to_skuffen: HashMap<Uuid, Uuid>,
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
        _client_reference: Uuid,
        _arkiv_id: String,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn hent_arkiv_id_fra_mapping(
        &self,
        _skuffen_id: SkuffenSakId,
    ) -> Result<Option<String>, anyhow::Error> {
        Ok(None)
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
        _arkiv_id: &str,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error> {
        Ok(None)
    }
    async fn hent_eller_opprett_skuffen_id_for_arkiv_id(
        &self,
        _entity_type: MappingEntityType,
        _arkiv_id: &str,
    ) -> Result<SkuffenSakId, anyhow::Error> {
        unreachable!()
    }
    async fn delete_arkiv_mapping(
        &self,
        _entity_type: MappingEntityType,
        _arkiv_id: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn etter_sak_endret_gjor_journalpostkommando_klar_nar_saksnummer_kommer() {
    let execution_repo = FakeExecutionRepository::default();
    let snapshot_repo = FakeSnapshotRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let dokument_client_reference = Uuid::new_v4();
    let dokument_id = Uuid::new_v4();
    let command_id = Uuid::new_v4();

    {
        let mut data = id_mapping_repo.data.lock().unwrap();
        data.client_to_skuffen.insert(sak_client_reference, sak_id);
        data.client_to_skuffen
            .insert(journalpost_client_reference, journalpost_id);
        data.client_to_skuffen
            .insert(dokument_client_reference, dokument_id);
    }

    snapshot_repo.data.lock().unwrap().saker.insert(
        sak_id,
        SakState {
            status: SakStatus::UnderBehandling,
            opprettet: true,
            saksnummer: Some("2026/1".to_string()),
        },
    );

    execution_repo.data.lock().unwrap().ventende_for_sak.insert(
        sak_id,
        vec![EksekveringKommando {
            command_id,
            envelope: make_internt_notat_command(
                journalpost_client_reference,
                sak_client_reference,
                dokument_client_reference,
            ),
            attempt_no: 0,
            utfores_venter_publisert: true,
        }],
    );

    let service = ReevaluerVentendeKommandoerService::new(
        Box::new(execution_repo.clone()),
        Box::new(snapshot_repo),
        Box::new(id_mapping_repo),
        Box::new(status_publisher),
        Box::new(done_publisher),
        Box::new(FakeStatusProjector),
    );

    service
        .etter_sak_endret(SkuffenSakId::from(sak_id))
        .await
        .unwrap();

    let data = execution_repo.data.lock().unwrap();
    assert_eq!(data.oppdatert_til_klar, vec![command_id]);
    assert!(data.oppdatert_til_venter.is_empty());
    assert!(data.oppdatert_til_feil.is_empty());
}

#[tokio::test]
async fn etter_sak_endret_gjor_avslutt_sak_til_feil_ved_feilede_dokumenter() {
    let execution_repo = FakeExecutionRepository::default();
    let snapshot_repo = FakeSnapshotRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let command_id = Uuid::new_v4();

    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .client_to_skuffen
        .insert(sak_client_reference, sak_id);

    {
        let mut data = snapshot_repo.data.lock().unwrap();
        data.saker.insert(
            sak_id,
            SakState {
                status: SakStatus::UnderBehandling,
                opprettet: true,
                saksnummer: Some("2026/1".to_string()),
            },
        );
        data.journalposter.insert(
            journalpost_id,
            JournalpostState {
                journalfoert: true,
                avskrevet: false,
                ekspedert: false,
                har_feilede_dokumenter: true,
                med_utsending: false,
                journalposttype: JournalpostType::InterntNotat,
                journalpostnummer: Some(42),
            },
        );
        data.journalposter_per_sak
            .insert(sak_id, vec![journalpost_id]);
    }

    execution_repo.data.lock().unwrap().ventende_for_sak.insert(
        sak_id,
        vec![EksekveringKommando {
            command_id,
            envelope: make_avslutt_sak_command(sak_client_reference),
            attempt_no: 0,
            utfores_venter_publisert: true,
        }],
    );

    let service = ReevaluerVentendeKommandoerService::new(
        Box::new(execution_repo.clone()),
        Box::new(snapshot_repo),
        Box::new(id_mapping_repo),
        Box::new(status_publisher.clone()),
        Box::new(done_publisher.clone()),
        Box::new(FakeStatusProjector),
    );

    service
        .etter_sak_endret(SkuffenSakId::from(sak_id))
        .await
        .unwrap();

    let data = execution_repo.data.lock().unwrap();
    assert_eq!(data.oppdatert_til_feil.len(), 1);
    assert_eq!(data.oppdatert_til_feil[0].0, command_id);
    assert!(data.oppdatert_til_feil[0].1.contains("feilet"));
    assert!(data.oppdatert_til_klar.is_empty());
    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].terminal);
    assert_eq!(done_publisher.subjects.lock().unwrap().len(), 1);
}

fn make_internt_notat_command(
    journalpost_client_reference: Uuid,
    sak_client_reference: Uuid,
    dokument_client_reference: Uuid,
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
                dokumenter: vec![Dokument {
                    client_reference: dokument_client_reference,
                    tittel: "Vedlegg".to_string(),
                    filtype: "PDF".to_string(),
                    dokument_referanse: Uuid::new_v4(),
                }],
                sak_key: SakKey::ClientReference(sak_client_reference),
                kildesystem: None,
            },
        }),
    }
}

fn make_avslutt_sak_command(sak_client_reference: Uuid) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::AvsluttSak(AvsluttSak {
            sak_key: SakKey::ClientReference(sak_client_reference),
        }),
    }
}
