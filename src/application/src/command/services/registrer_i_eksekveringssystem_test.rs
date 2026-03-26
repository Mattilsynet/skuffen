use async_trait::async_trait;
use domain::eksekvering::plan::JournalpostType;
use domain::eksekvering::typer::{
    CommandLifecycleContext, CommandLifecycleEvent, CommandStage, CommandStageStatus,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::dokument::Dokument;
use lib_schemas::skuffen::query::queries::SakKey;
use lib_schemas::skuffen::sak::{Ordningsverdi, Saksnummer, Sakstittel};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::command::ports::eksekvering_port::EksekveringStatusPublisher;
use crate::command::ports::eksekvering_state_port::{
    DokumentState, EksekveringKommando, EksekveringStateRepository, EksekveringStatus,
    EksekveringsregistreringResultat, EksekveringssystemRegistration,
    JournalpostOpprettetTransition, JournalpostOvergangVedJournalfoering, JournalpostState,
    SakState, SakStatus, SakTransition,
};
use crate::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};
use crate::command::ports::registrer_i_eksekveringssystem_port::RegistrerIEksekveringssystemUseCase;
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::services::registrer_i_eksekveringssystem::RegistrerIEksekveringssystemService;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};

#[derive(Clone, Default)]
struct FakeEksekveringStateRepository {
    data: Arc<Mutex<FakeStateData>>,
}

struct FakeStateData {
    sak_state: Option<(Uuid, SakState)>,
    journalpost_state: Option<(Uuid, Uuid, JournalpostState)>,
    registrerte_kommandoer: Vec<Uuid>,
    markerte_utfores_venter: Vec<Uuid>,
    existing_sak: Option<SakState>,
    existing_journalpost: Option<JournalpostState>,
    existing_dokument: Option<DokumentState>,
    registrering_resultat: EksekveringsregistreringResultat,
}

impl Default for FakeStateData {
    fn default() -> Self {
        Self {
            sak_state: None,
            journalpost_state: None,
            registrerte_kommandoer: Vec::new(),
            markerte_utfores_venter: Vec::new(),
            existing_sak: None,
            existing_journalpost: None,
            existing_dokument: None,
            registrering_resultat: EksekveringsregistreringResultat::Nyregistrert,
        }
    }
}

#[async_trait]
impl EksekveringStateRepository for FakeEksekveringStateRepository {
    async fn hent_sak_state(
        &self,
        _sak_id: SkuffenSakId,
    ) -> Result<Option<SakState>, anyhow::Error> {
        Ok(self.data.lock().unwrap().existing_sak.clone())
    }

    async fn ensure_sak_state(
        &self,
        sak_id: SkuffenSakId,
        state: SakState,
    ) -> Result<SakState, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        if let Some(existing) = data.existing_sak.clone() {
            return Ok(existing);
        }
        data.sak_state = Some((Uuid::from(sak_id), state.clone()));
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
        self.data.lock().unwrap().sak_state = Some((Uuid::from(sak_id), state.clone()));
        Ok(state)
    }

    async fn hent_journalpost_state(
        &self,
        _journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<JournalpostState>, anyhow::Error> {
        Ok(self.data.lock().unwrap().existing_journalpost.clone())
    }

    async fn ensure_journalpost_state(
        &self,
        journalpost_id: SkuffenJournalpostId,
        sak_id: SkuffenSakId,
        state: JournalpostState,
    ) -> Result<JournalpostState, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        if let Some(existing) = data.existing_journalpost.clone() {
            return Ok(existing);
        }
        data.journalpost_state = Some((
            Uuid::from(journalpost_id),
            Uuid::from(sak_id),
            state.clone(),
        ));
        Ok(state)
    }

    async fn anvend_journalpost_opprettet(
        &self,
        journalpost_id: SkuffenJournalpostId,
        transition: JournalpostOpprettetTransition,
    ) -> Result<JournalpostState, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        if let Some((_, sak_id, mut state)) = data.journalpost_state.clone() {
            state.journalpostnummer = Some(transition.journalpostnummer);
            data.journalpost_state = Some((Uuid::from(journalpost_id), sak_id, state.clone()));
            return Ok(state);
        }
        let mut state = data
            .existing_journalpost
            .clone()
            .unwrap_or(JournalpostState {
                journalfoert: false,
                avskrevet: false,
                ekspedert: false,
                har_feilede_dokumenter: false,
                med_utsending: false,
                journalposttype: JournalpostType::InterntNotat,
                journalpostnummer: None,
            });
        state.journalpostnummer = Some(transition.journalpostnummer);
        Ok(state)
    }

    async fn anvend_journalpost_overgang_ved_journalfoering(
        &self,
        _journalpost_id: SkuffenJournalpostId,
        _transition: JournalpostOvergangVedJournalfoering,
    ) -> Result<JournalpostState, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .existing_journalpost
            .clone()
            .unwrap_or(JournalpostState {
                journalfoert: true,
                avskrevet: false,
                ekspedert: false,
                har_feilede_dokumenter: false,
                med_utsending: false,
                journalposttype: JournalpostType::InterntNotat,
                journalpostnummer: Some(1),
            }))
    }

    async fn anvend_journalpost_avskrevet(
        &self,
        _journalpost_id: SkuffenJournalpostId,
    ) -> Result<JournalpostState, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .existing_journalpost
            .clone()
            .unwrap_or(JournalpostState {
                journalfoert: true,
                avskrevet: true,
                ekspedert: false,
                har_feilede_dokumenter: false,
                med_utsending: false,
                journalposttype: JournalpostType::InterntNotat,
                journalpostnummer: Some(1),
            }))
    }

    async fn hent_journalposter_for_sak(
        &self,
        _sak_id: SkuffenSakId,
    ) -> Result<Vec<JournalpostState>, anyhow::Error> {
        Ok(Vec::new())
    }

    async fn hent_dokument_state(
        &self,
        _dokument_id: SkuffenDokumentId,
    ) -> Result<Option<DokumentState>, anyhow::Error> {
        Ok(None)
    }

    async fn ensure_dokument_state(
        &self,
        _dokument_id: SkuffenDokumentId,
        _journalpost_id: SkuffenJournalpostId,
        state: DokumentState,
    ) -> Result<DokumentState, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .existing_dokument
            .clone()
            .unwrap_or(state))
    }

    async fn anvend_dokument_lagt_til(
        &self,
        _dokument_id: SkuffenDokumentId,
        _journalpost_id: SkuffenJournalpostId,
    ) -> Result<DokumentState, anyhow::Error> {
        Ok(DokumentState {
            lagt_til: true,
            irrecoverable_feil: false,
        })
    }

    async fn oppdater_eksekvering(
        &self,
        _command_id: Uuid,
        _status: EksekveringStatus,
        _last_error: Option<String>,
        _next_retry_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn registrer_kommando(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<bool, anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .registrerte_kommandoer
            .push(envelope.command_id);
        Ok(true)
    }

    async fn ensure_registrert_i_eksekveringssystem(
        &self,
        registration: &EksekveringssystemRegistration,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<EksekveringsregistreringResultat, anyhow::Error> {
        if let Some(sak) = &registration.sak {
            let _ = self.ensure_sak_state(sak.sak_id, sak.state.clone()).await?;
        }

        if let Some(journalpost) = &registration.journalpost {
            let _ = self
                .ensure_journalpost_state(
                    journalpost.journalpost_id,
                    journalpost.sak_id,
                    journalpost.state.clone(),
                )
                .await?;
        }

        let mut data = self.data.lock().unwrap();
        data.registrerte_kommandoer.push(envelope.command_id);
        Ok(data.registrering_resultat)
    }

    async fn marker_utfores_venter_publisert(&self, command_id: Uuid) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .markerte_utfores_venter
            .push(command_id);
        Ok(())
    }

    async fn hent_klare_kommandoer(
        &self,
        _limit: i64,
        _worker_id: &str,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error> {
        Ok(Vec::new())
    }
}

#[derive(Clone, Default)]
struct FakeIdMappingRepository {
    data: Arc<Mutex<FakeIdMappingData>>,
}

#[derive(Default)]
struct FakeIdMappingData {
    ensure_calls: Vec<(String, String)>,
    ensured_sak_id: Option<Uuid>,
    skuffen_id_for_client_reference: Vec<(Uuid, Uuid)>,
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
            .skuffen_id_for_client_reference
            .iter()
            .find_map(|(stored_client_reference, skuffen_id)| {
                (*stored_client_reference == client_reference)
                    .then_some(SkuffenSakId::from(*skuffen_id))
            }))
    }

    async fn hent_journalpost_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenJournalpostId>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .skuffen_id_for_client_reference
            .iter()
            .find_map(|(stored_client_reference, skuffen_id)| {
                (*stored_client_reference == client_reference)
                    .then_some(SkuffenJournalpostId::from(*skuffen_id))
            }))
    }

    async fn hent_dokument_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenDokumentId>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .skuffen_id_for_client_reference
            .iter()
            .find_map(|(stored_client_reference, skuffen_id)| {
                (*stored_client_reference == client_reference)
                    .then_some(SkuffenDokumentId::from(*skuffen_id))
            }))
    }

    async fn hent_sak_id_fra_arkiv_id_i_mapping(
        &self,
        _arkiv_id: &str,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_eller_opprett_skuffen_id_for_arkiv_id(
        &self,
        entity_type: MappingEntityType,
        arkiv_id: &str,
    ) -> Result<SkuffenSakId, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        data.ensure_calls
            .push((entity_type.as_code().to_string(), arkiv_id.to_string()));
        Ok(SkuffenSakId::from(
            data.ensured_sak_id.unwrap_or_else(Uuid::new_v4),
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

#[derive(Clone, Default)]
struct FakeStatusPublisher {
    events: Arc<Mutex<Vec<CommandLifecycleEvent>>>,
    fail_next: Arc<Mutex<bool>>,
}

#[async_trait]
impl EksekveringStatusPublisher for FakeStatusPublisher {
    async fn publiser_status(&self, event: CommandLifecycleEvent) -> Result<(), anyhow::Error> {
        let mut fail_next = self.fail_next.lock().unwrap();
        if *fail_next {
            *fail_next = false;
            return Err(anyhow::anyhow!("publish failed"));
        }
        self.events.lock().unwrap().push(event);
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

fn build_service(
    state_repo: FakeEksekveringStateRepository,
    id_mapping_repo: FakeIdMappingRepository,
    status_publisher: FakeStatusPublisher,
) -> RegistrerIEksekveringssystemService {
    RegistrerIEksekveringssystemService::new(
        Box::new(state_repo),
        Box::new(id_mapping_repo),
        Box::new(status_publisher),
        Box::new(FakeStatusContextResolver),
    )
}

#[tokio::test]
async fn registrer_i_eksekveringssystem_seeds_state_for_client_reference_sak() {
    let state_repo = FakeEksekveringStateRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let service = build_service(
        state_repo.clone(),
        id_mapping_repo.clone(),
        status_publisher.clone(),
    );

    let sak_id = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    {
        let mut data = id_mapping_repo.data.lock().unwrap();
        data.skuffen_id_for_client_reference = vec![
            (sak_id, sak_skuffen_id),
            (journalpost_id, journalpost_skuffen_id),
        ];
    }
    let envelope = make_journalpost_command(journalpost_id, SakKey::ClientReference(sak_id));

    service.handle(&envelope).await.unwrap();

    let state = state_repo.data.lock().unwrap();
    let (saved_sak_id, saved_sak_state) = state.sak_state.clone().unwrap();
    assert_eq!(saved_sak_id, sak_skuffen_id);
    assert_eq!(saved_sak_state.status, SakStatus::UnderBehandling);
    assert!(!saved_sak_state.opprettet);
    assert_eq!(saved_sak_state.saksnummer, None);

    let (saved_journalpost_id, linked_sak_id, saved_journalpost_state) =
        state.journalpost_state.clone().unwrap();
    assert_eq!(saved_journalpost_id, journalpost_skuffen_id);
    assert_eq!(linked_sak_id, sak_skuffen_id);
    assert_eq!(
        saved_journalpost_state.journalposttype,
        JournalpostType::InterntNotat
    );
    assert_eq!(state.registrerte_kommandoer, vec![envelope.command_id]);
    assert_eq!(state.markerte_utfores_venter, vec![envelope.command_id]);
    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage, CommandStage::Utfores);
    assert_eq!(events[0].stage_status, CommandStageStatus::Venter);
    assert_eq!(events[0].message, "utfores::venter");

    let id_mapping = id_mapping_repo.data.lock().unwrap();
    assert!(id_mapping.ensure_calls.is_empty());
}

#[tokio::test]
async fn registrer_i_eksekveringssystem_ensures_mapping_for_arkiv_id_sak() {
    let state_repo = FakeEksekveringStateRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let ensured_sak_id = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    id_mapping_repo.data.lock().unwrap().ensured_sak_id = Some(ensured_sak_id);
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![(journalpost_id, journalpost_skuffen_id)];

    let service = build_service(
        state_repo.clone(),
        id_mapping_repo.clone(),
        status_publisher.clone(),
    );

    let envelope = make_journalpost_command(
        journalpost_id,
        SakKey::ArkivId(lib_schemas::skuffen::sak::Saksnummer::new("2025/123").unwrap()),
    );

    service.handle(&envelope).await.unwrap();

    let state = state_repo.data.lock().unwrap();
    let (saved_sak_id, saved_sak_state) = state.sak_state.clone().unwrap();
    assert_eq!(saved_sak_id, ensured_sak_id);
    assert!(saved_sak_state.opprettet);
    assert_eq!(saved_sak_state.saksnummer.as_deref(), Some("2025/123"));

    let (saved_journalpost_id, linked_sak_id, _) = state.journalpost_state.clone().unwrap();
    assert_eq!(saved_journalpost_id, journalpost_skuffen_id);
    assert_eq!(linked_sak_id, ensured_sak_id);
    drop(state);

    let id_mapping = id_mapping_repo.data.lock().unwrap();
    assert_eq!(
        id_mapping.ensure_calls,
        vec![("sak".to_string(), "2025/123".to_string())]
    );
    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].message, "utfores::venter");
    drop(events);

    let state = state_repo.data.lock().unwrap();
    assert_eq!(state.markerte_utfores_venter, vec![envelope.command_id]);
}

#[tokio::test]
async fn registrer_i_eksekveringssystem_seeds_state_for_opprett_sak() {
    let state_repo = FakeEksekveringStateRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let service = build_service(
        state_repo.clone(),
        id_mapping_repo.clone(),
        status_publisher.clone(),
    );

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![(sak_client_reference, sak_skuffen_id)];

    let envelope = make_opprett_sak_command(sak_client_reference);

    service.handle(&envelope).await.unwrap();

    let state = state_repo.data.lock().unwrap();
    let (saved_sak_id, saved_sak_state) = state.sak_state.clone().unwrap();
    assert_eq!(saved_sak_id, sak_skuffen_id);
    assert_eq!(saved_sak_state.status, SakStatus::UnderBehandling);
    assert!(!saved_sak_state.opprettet);
    assert_eq!(saved_sak_state.saksnummer, None);
    assert_eq!(state.registrerte_kommandoer, vec![envelope.command_id]);
    assert_eq!(state.markerte_utfores_venter, vec![envelope.command_id]);

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].message, "utfores::venter");
}

#[tokio::test]
async fn registrer_i_eksekveringssystem_seeds_state_for_avslutt_sak_med_client_reference() {
    let state_repo = FakeEksekveringStateRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let service = build_service(
        state_repo.clone(),
        id_mapping_repo.clone(),
        status_publisher.clone(),
    );

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![(sak_client_reference, sak_skuffen_id)];

    let envelope = make_avslutt_sak_command(SakKey::ClientReference(sak_client_reference));

    service.handle(&envelope).await.unwrap();

    let state = state_repo.data.lock().unwrap();
    let (saved_sak_id, saved_sak_state) = state.sak_state.clone().unwrap();
    assert_eq!(saved_sak_id, sak_skuffen_id);
    assert_eq!(saved_sak_state.status, SakStatus::UnderBehandling);
    assert!(!saved_sak_state.opprettet);
    assert_eq!(saved_sak_state.saksnummer, None);
    assert_eq!(state.registrerte_kommandoer, vec![envelope.command_id]);
    assert_eq!(state.markerte_utfores_venter, vec![envelope.command_id]);

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].message, "utfores::venter");
}

#[tokio::test]
async fn registrer_i_eksekveringssystem_seeds_state_for_avslutt_sak_med_arkiv_id() {
    let state_repo = FakeEksekveringStateRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let ensured_sak_id = Uuid::new_v4();
    id_mapping_repo.data.lock().unwrap().ensured_sak_id = Some(ensured_sak_id);

    let service = build_service(
        state_repo.clone(),
        id_mapping_repo.clone(),
        status_publisher.clone(),
    );

    let envelope = make_avslutt_sak_command(SakKey::ArkivId(Saksnummer::new("2025/456").unwrap()));

    service.handle(&envelope).await.unwrap();

    let state = state_repo.data.lock().unwrap();
    let (saved_sak_id, saved_sak_state) = state.sak_state.clone().unwrap();
    assert_eq!(saved_sak_id, ensured_sak_id);
    assert_eq!(saved_sak_state.status, SakStatus::UnderBehandling);
    assert!(saved_sak_state.opprettet);
    assert_eq!(saved_sak_state.saksnummer.as_deref(), Some("2025/456"));
    assert_eq!(state.registrerte_kommandoer, vec![envelope.command_id]);
    assert_eq!(state.markerte_utfores_venter, vec![envelope.command_id]);

    let id_mapping = id_mapping_repo.data.lock().unwrap();
    assert_eq!(
        id_mapping.ensure_calls,
        vec![("sak".to_string(), "2025/456".to_string())]
    );

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].message, "utfores::venter");
}

#[tokio::test]
async fn registrer_i_eksekveringssystem_hopper_over_utfores_venter_nar_den_allerede_er_publisert() {
    let state_repo = FakeEksekveringStateRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let service = build_service(
        state_repo.clone(),
        id_mapping_repo.clone(),
        status_publisher.clone(),
    );

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![(sak_client_reference, sak_skuffen_id)];
    state_repo.data.lock().unwrap().registrering_resultat =
        EksekveringsregistreringResultat::EksisterteMedVenterPublisert;

    let envelope = make_opprett_sak_command(sak_client_reference);

    service.handle(&envelope).await.unwrap();

    let state = state_repo.data.lock().unwrap();
    assert_eq!(state.registrerte_kommandoer, vec![envelope.command_id]);
    drop(state);

    let events = status_publisher.events.lock().unwrap();
    assert!(events.is_empty());
    drop(events);

    let state = state_repo.data.lock().unwrap();
    assert!(state.markerte_utfores_venter.is_empty());
}

#[tokio::test]
async fn registrer_i_eksekveringssystem_publiserer_utfores_venter_pa_replay_nar_den_mangler() {
    let state_repo = FakeEksekveringStateRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let service = build_service(
        state_repo.clone(),
        id_mapping_repo.clone(),
        status_publisher.clone(),
    );

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![(sak_client_reference, sak_skuffen_id)];
    state_repo.data.lock().unwrap().registrering_resultat =
        EksekveringsregistreringResultat::EksisterteUtenVenterPublisert;

    let envelope = make_opprett_sak_command(sak_client_reference);

    service.handle(&envelope).await.unwrap();

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].message, "utfores::venter");
    drop(events);

    let state = state_repo.data.lock().unwrap();
    assert_eq!(state.markerte_utfores_venter, vec![envelope.command_id]);
}

#[tokio::test]
async fn registrer_i_eksekveringssystem_keeps_existing_state_unchanged_on_reregistration() {
    let state_repo = FakeEksekveringStateRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let service = build_service(
        state_repo.clone(),
        id_mapping_repo.clone(),
        status_publisher.clone(),
    );

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    {
        let mut data = id_mapping_repo.data.lock().unwrap();
        data.skuffen_id_for_client_reference = vec![
            (sak_client_reference, sak_skuffen_id),
            (journalpost_client_reference, journalpost_skuffen_id),
        ];
    }
    {
        let mut data = state_repo.data.lock().unwrap();
        data.existing_sak = Some(SakState {
            status: SakStatus::UnderBehandling,
            opprettet: true,
            saksnummer: Some("2026/1".to_string()),
        });
        data.existing_journalpost = Some(JournalpostState {
            journalfoert: true,
            avskrevet: false,
            ekspedert: false,
            har_feilede_dokumenter: false,
            med_utsending: false,
            journalposttype: JournalpostType::InterntNotat,
            journalpostnummer: Some(42),
        });
        data.registrering_resultat = EksekveringsregistreringResultat::EksisterteMedVenterPublisert;
    }

    let envelope = make_journalpost_command(
        journalpost_client_reference,
        SakKey::ClientReference(sak_client_reference),
    );

    service.handle(&envelope).await.unwrap();

    let state = state_repo.data.lock().unwrap();
    assert!(state.sak_state.is_none());
    assert!(state.journalpost_state.is_none());
    assert_eq!(state.registrerte_kommandoer, vec![envelope.command_id]);
    drop(state);

    let events = status_publisher.events.lock().unwrap();
    assert!(events.is_empty());
}

#[tokio::test]
async fn registrer_i_eksekveringssystem_markerer_ikke_utfores_venter_nar_publisering_feiler() {
    let state_repo = FakeEksekveringStateRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let service = build_service(
        state_repo.clone(),
        id_mapping_repo.clone(),
        status_publisher.clone(),
    );

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![(sak_client_reference, sak_skuffen_id)];
    *status_publisher.fail_next.lock().unwrap() = true;

    let envelope = make_opprett_sak_command(sak_client_reference);

    let err = service.handle(&envelope).await.unwrap_err();

    assert!(err.to_string().contains("publish failed"));
    assert!(status_publisher.events.lock().unwrap().is_empty());
    let state = state_repo.data.lock().unwrap();
    assert!(state.markerte_utfores_venter.is_empty());
}

fn make_journalpost_command(journalpost_id: Uuid, sak_key: SakKey) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
            felles: JournalpostCommon {
                client_reference: journalpost_id,
                tittel: "Internt notat".to_string(),
                dokument_dato: "2025-01-01".to_string(),
                saksbehandler: "Z12345".to_string(),
                saksbehandler_enhet: "42".to_string(),
                tilgang: None,
                dokumenter: vec![Dokument {
                    client_reference: Uuid::new_v4(),
                    tittel: "Vedlegg".to_string(),
                    filtype: "PDF".to_string(),
                    dokument_referanse: Uuid::new_v4(),
                }],
                sak_key,
                kildesystem: None,
            },
        }),
    }
}

fn make_opprett_sak_command(sak_client_reference: Uuid) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettSak(OpprettSak {
            client_reference: sak_client_reference,
            sakstittel: Sakstittel("Test sak".to_string()),
            ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
            arkivdel: Arkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: None,
        }),
    }
}

fn make_avslutt_sak_command(sak_key: SakKey) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::AvsluttSak(AvsluttSak { sak_key }),
    }
}
