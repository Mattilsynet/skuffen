use async_trait::async_trait;
use domain::eksekvering::execution::EksekveringStatus;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::JournalpostType;
use domain::eksekvering::tilstand::{
    DokumentKildeTilstand, DokumentMedTilstand, DokumentTilstand, JournalpostMedDokumenter,
    JournalpostTilstand, SakMedBarn, SakTilstand,
};
use domain::eksekvering::typer::{
    CommandLifecycleContext, CommandLifecycleEvent, CommandStage, CommandStageStatus,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::dokument::{Dokument, Dokumentform, Felt};
use lib_schemas::skuffen::query::queries::SakKey;
use lib_schemas::skuffen::sak::{Ordningsverdi, Saksnummer, Sakstittel};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::command::ports::command_execution_port::{
    CommandExecutionRepository, EksekveringKommando, EksekveringsregistreringResultat,
    NyKommandoEksekvering,
};
use crate::command::ports::eksekvering_port::EksekveringStatusPublisher;
use crate::command::ports::entity_tilstand_port::EntityTilstandRepository;
use crate::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};
use crate::command::ports::registrer_i_eksekveringssystem_port::RegistrerIEksekveringssystemUseCase;
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::services::registrer_i_eksekveringssystem::RegistrerIEksekveringssystemService;

// ---------------------------------------------------------------------------
// FakeEntityTilstandRepository
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FakeEntityTilstandData {
    opprettede_saker: Vec<(Uuid, SakTilstand, Uuid)>,
    opprettede_journalposter: Vec<(Uuid, Uuid, JournalpostType, bool, JournalpostTilstand, Uuid)>,
    opprettede_dokumenter: Vec<(Uuid, Uuid, DokumentTilstand, Option<Uuid>, Vec<Felt>, Uuid)>,
    oppdaterte_sak_oensket: Vec<(Uuid, SakTilstand)>,
    sak_med_barn: HashMap<Uuid, SakMedBarn>,
}

#[derive(Clone, Default)]
struct FakeEntityTilstandRepository {
    data: Arc<Mutex<FakeEntityTilstandData>>,
}

#[async_trait]
impl EntityTilstandRepository for FakeEntityTilstandRepository {
    async fn opprett_sak_tilstand(
        &self,
        sak_id: SkuffenSakId,
        oensket_tilstand: SakTilstand,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        self.data.lock().unwrap().opprettede_saker.push((
            Uuid::from(sak_id),
            oensket_tilstand,
            command_id,
        ));
        Ok(())
    }

    async fn oppdater_sak_tilstand(
        &self,
        _sak_id: SkuffenSakId,
        _tilstand: SakTilstand,
        _sikri_id: Option<i64>,
        _saksnummer: Option<&str>,
        _feil_detalj: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn oppdater_sak_oensket_tilstand(
        &self,
        sak_id: SkuffenSakId,
        oensket_tilstand: SakTilstand,
    ) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .oppdaterte_sak_oensket
            .push((Uuid::from(sak_id), oensket_tilstand));
        Ok(())
    }

    async fn oppdater_oensket_saksansvarlig(
        &self,
        _sak_id: SkuffenSakId,
        _saksbehandler_id: &str,
        _saksbehandler_enhet: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn oppdater_naavaerende_saksansvarlig(
        &self,
        _sak_id: SkuffenSakId,
        _saksbehandler_id: &str,
        _saksbehandler_enhet: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn opprett_journalpost_tilstand(
        &self,
        journalpost_id: SkuffenJournalpostId,
        sak_id: SkuffenSakId,
        journalposttype: JournalpostType,
        med_utsending: bool,
        oensket_tilstand: JournalpostTilstand,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        self.data.lock().unwrap().opprettede_journalposter.push((
            Uuid::from(journalpost_id),
            Uuid::from(sak_id),
            journalposttype,
            med_utsending,
            oensket_tilstand,
            command_id,
        ));
        Ok(())
    }

    async fn oppdater_journalpost_tilstand(
        &self,
        _journalpost_id: SkuffenJournalpostId,
        _tilstand: JournalpostTilstand,
        _sikri_id: Option<i64>,
        _journalpostnummer: Option<i32>,
        _feil_detalj: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn opprett_dokument_tilstand(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
        tilstand: DokumentTilstand,
        mal_referanse: Option<Uuid>,
        felter: Vec<Felt>,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        self.data.lock().unwrap().opprettede_dokumenter.push((
            Uuid::from(dokument_id),
            Uuid::from(journalpost_id),
            tilstand,
            mal_referanse,
            felter,
            command_id,
        ));
        Ok(())
    }

    async fn oppdater_dokument_tilstand(
        &self,
        _dokument_id: SkuffenDokumentId,
        _tilstand: DokumentTilstand,
        _feil_detalj: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn oppdater_rendered_dokument_referanse(
        &self,
        _dokument_id: SkuffenDokumentId,
        _rendered_dokument_referanse: Uuid,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn hent_sak_med_barn(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<SakMedBarn>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .sak_med_barn
            .get(&Uuid::from(sak_id))
            .cloned())
    }

    async fn logg_overgang(
        &self,
        _entity_type: &str,
        _entity_id: Uuid,
        _command_id: Uuid,
        _fra_tilstand: &str,
        _til_tilstand: &str,
        _operasjon: &str,
        _feil_detalj: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// FakeExecutionRepository
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FakeExecutionData {
    opprettede_eksekveringer: Vec<(Uuid, EksekveringStatus, Option<String>)>,
    markerte_utfores_venter: Vec<Uuid>,
    registrering_resultat: Option<EksekveringsregistreringResultat>,
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
        ny: NyKommandoEksekvering,
    ) -> Result<EksekveringsregistreringResultat, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        data.opprettede_eksekveringer.push((
            ny.envelope.command_id,
            ny.status,
            ny.last_detail.clone(),
        ));
        Ok(data
            .registrering_resultat
            .unwrap_or(EksekveringsregistreringResultat::Nyregistrert))
    }

    async fn marker_utfores_venter_publisert(&self, command_id: Uuid) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .markerte_utfores_venter
            .push(command_id);
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

    async fn marker_blokkert_venter(
        &self,
        _command_id: Uuid,
        _attempt_no: i32,
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

    async fn hent_blokkert_venter_for_sak(
        &self,
        _sak_id: SkuffenSakId,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error> {
        Ok(Vec::new())
    }

    async fn marker_blokkert_venter_til_klar(
        &self,
        _command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn oppdater_til_klar(&self, _command_id: Uuid) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn oppdater_til_feil(
        &self,
        _command_id: Uuid,
        _detalj: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn reset_kjorer_til_klar(&self) -> Result<u64, anyhow::Error> {
        Ok(0)
    }
}

// ---------------------------------------------------------------------------
// FakeIdMappingRepository
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FakeIdMappingData {
    ensure_calls: Vec<(String, String)>,
    ensured_sak_id: Option<Uuid>,
    skuffen_id_for_client_reference: Vec<(Uuid, Uuid)>,
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
            })
            .or_else(|| Some(SkuffenDokumentId::from(client_reference))))
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

// ---------------------------------------------------------------------------
// FakeStatusPublisher
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// FakeStatusContextResolver
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Build helpers
// ---------------------------------------------------------------------------

fn build_service(
    execution_repo: FakeExecutionRepository,
    entity_tilstand_repo: FakeEntityTilstandRepository,
    id_mapping_repo: FakeIdMappingRepository,
    status_publisher: FakeStatusPublisher,
) -> RegistrerIEksekveringssystemService {
    RegistrerIEksekveringssystemService::new(
        Box::new(execution_repo),
        Box::new(entity_tilstand_repo),
        Box::new(id_mapping_repo),
        Box::new(status_publisher),
        Box::new(FakeStatusContextResolver),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn opprett_sak_oppretter_tilstand_og_registrerer_som_klar() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![(sak_client_reference, sak_skuffen_id)];

    let service = build_service(
        execution_repo.clone(),
        entity_repo.clone(),
        id_mapping_repo,
        status_publisher.clone(),
    );

    let envelope = make_opprett_sak_command(sak_client_reference);
    service.handle(&envelope).await.unwrap();

    let entity_data = entity_repo.data.lock().unwrap();
    assert_eq!(entity_data.opprettede_saker.len(), 1);
    let (saved_sak_id, oensket, cmd_id) = &entity_data.opprettede_saker[0];
    assert_eq!(*saved_sak_id, sak_skuffen_id);
    assert_eq!(*oensket, SakTilstand::Opprettet);
    assert_eq!(*cmd_id, envelope.command_id);
    drop(entity_data);

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(envelope.command_id, EksekveringStatus::Klar, None)]
    );
    assert_eq!(exec_data.markerte_utfores_venter, vec![envelope.command_id]);
    drop(exec_data);

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage, CommandStage::Utfores);
    assert_eq!(events[0].stage_status, CommandStageStatus::Venter);
}

#[tokio::test]
async fn journalpost_med_client_reference_sak_ikke_i_tilstand_gir_blokkert_venter() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![
        (sak_client_reference, sak_skuffen_id),
        (journalpost_client_reference, journalpost_skuffen_id),
    ];

    let service = build_service(
        execution_repo.clone(),
        entity_repo.clone(),
        id_mapping_repo,
        status_publisher.clone(),
    );

    let envelope = make_journalpost_command(
        journalpost_client_reference,
        SakKey::ClientReference(sak_client_reference),
    );
    service.handle(&envelope).await.unwrap();

    let entity_data = entity_repo.data.lock().unwrap();
    assert_eq!(entity_data.opprettede_journalposter.len(), 1);
    let (jp_id, linked_sak_id, jp_type, _, oensket, cmd_id) =
        &entity_data.opprettede_journalposter[0];
    assert_eq!(*jp_id, journalpost_skuffen_id);
    assert_eq!(*linked_sak_id, sak_skuffen_id);
    assert_eq!(*jp_type, JournalpostType::InterntNotat);
    assert_eq!(*oensket, JournalpostTilstand::Journalfoert);
    assert_eq!(*cmd_id, envelope.command_id);
    assert_eq!(entity_data.opprettede_dokumenter.len(), 1);
    drop(entity_data);

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(envelope.command_id, EksekveringStatus::BlokkertVenter, None)]
    );
    assert_eq!(exec_data.markerte_utfores_venter, vec![envelope.command_id]);
    drop(exec_data);

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage_status, CommandStageStatus::Venter);
}

#[tokio::test]
async fn journalpost_med_arkiv_id_sak_opprettet_gir_klar() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    let ensured_sak_id = Uuid::new_v4();

    id_mapping_repo.data.lock().unwrap().ensured_sak_id = Some(ensured_sak_id);
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference =
        vec![(journalpost_client_reference, journalpost_skuffen_id)];

    // Sak allerede Opprettet med saksnummer, jp er IkkeRealisert
    entity_repo.data.lock().unwrap().sak_med_barn.insert(
        ensured_sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(ensured_sak_id),
            tilstand: SakTilstand::Opprettet,
            oensket_tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: Some("2025/123".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![JournalpostMedDokumenter {
                journalpost_id: SkuffenJournalpostId::from(journalpost_skuffen_id),
                tilstand: JournalpostTilstand::IkkeRealisert,
                oensket_tilstand: JournalpostTilstand::Journalfoert,
                sikri_id: None,
                journalpostnummer: None,
                journalposttype: JournalpostType::InterntNotat,
                med_utsending: false,
                dokumenter: vec![DokumentMedTilstand {
                    dokument_id: SkuffenDokumentId::from(Uuid::new_v4()),
                    tilstand: DokumentTilstand::IkkeRealisert,
                    kilde: DokumentKildeTilstand::Bytes,
                }],
            }],
        },
    );

    let service = build_service(
        execution_repo.clone(),
        entity_repo.clone(),
        id_mapping_repo.clone(),
        status_publisher.clone(),
    );

    let envelope = make_journalpost_command(
        journalpost_client_reference,
        SakKey::ArkivId(Saksnummer::new("2025/123").unwrap()),
    );
    service.handle(&envelope).await.unwrap();

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(envelope.command_id, EksekveringStatus::Klar, None)]
    );
    drop(exec_data);

    let id_mapping_data = id_mapping_repo.data.lock().unwrap();
    assert_eq!(
        id_mapping_data.ensure_calls,
        vec![("sak".to_string(), "2025/123".to_string())]
    );
    drop(id_mapping_data);

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage_status, CommandStageStatus::Venter);
}

#[tokio::test]
async fn avslutt_sak_oppdaterer_oensket_tilstand() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![(sak_client_reference, sak_skuffen_id)];

    // Sak ikke i tilstand-tabell → BlokkertVenter
    let service = build_service(
        execution_repo.clone(),
        entity_repo.clone(),
        id_mapping_repo,
        status_publisher.clone(),
    );

    let envelope = make_avslutt_sak_command(SakKey::ClientReference(sak_client_reference));
    service.handle(&envelope).await.unwrap();

    let entity_data = entity_repo.data.lock().unwrap();
    assert_eq!(entity_data.oppdaterte_sak_oensket.len(), 1);
    let (updated_sak_id, oensket) = &entity_data.oppdaterte_sak_oensket[0];
    assert_eq!(*updated_sak_id, sak_skuffen_id);
    assert_eq!(*oensket, SakTilstand::Avsluttet);
    drop(entity_data);

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(envelope.command_id, EksekveringStatus::BlokkertVenter, None)]
    );
    assert_eq!(exec_data.markerte_utfores_venter, vec![envelope.command_id]);
}

#[tokio::test]
async fn avslutt_sak_med_arkiv_id_og_opprettet_sak_gir_klar() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let ensured_sak_id = Uuid::new_v4();
    id_mapping_repo.data.lock().unwrap().ensured_sak_id = Some(ensured_sak_id);

    // Sak Opprettet med saksnummer, ingen journalposter
    entity_repo.data.lock().unwrap().sak_med_barn.insert(
        ensured_sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(ensured_sak_id),
            tilstand: SakTilstand::Opprettet,
            oensket_tilstand: SakTilstand::Avsluttet,
            sikri_id: Some(100),
            saksnummer: Some("2025/456".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: Vec::new(),
        },
    );

    let service = build_service(
        execution_repo.clone(),
        entity_repo.clone(),
        id_mapping_repo.clone(),
        status_publisher.clone(),
    );

    let envelope = make_avslutt_sak_command(SakKey::ArkivId(Saksnummer::new("2025/456").unwrap()));
    service.handle(&envelope).await.unwrap();

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(envelope.command_id, EksekveringStatus::Klar, None)]
    );
    assert_eq!(exec_data.markerte_utfores_venter, vec![envelope.command_id]);
    drop(exec_data);

    let id_mapping_data = id_mapping_repo.data.lock().unwrap();
    assert_eq!(
        id_mapping_data.ensure_calls,
        vec![("sak".to_string(), "2025/456".to_string())]
    );
    drop(id_mapping_data);

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage_status, CommandStageStatus::Venter);
}

#[tokio::test]
async fn hopper_over_utfores_venter_nar_allerede_publisert() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![(sak_client_reference, sak_skuffen_id)];
    execution_repo.data.lock().unwrap().registrering_resultat =
        Some(EksekveringsregistreringResultat::EksisterteMedVenterPublisert);

    let service = build_service(
        execution_repo.clone(),
        entity_repo,
        id_mapping_repo,
        status_publisher.clone(),
    );

    let envelope = make_opprett_sak_command(sak_client_reference);
    service.handle(&envelope).await.unwrap();

    let exec_data = execution_repo.data.lock().unwrap();
    assert!(exec_data.markerte_utfores_venter.is_empty());
    drop(exec_data);

    let events = status_publisher.events.lock().unwrap();
    assert!(events.is_empty());
}

#[tokio::test]
async fn publiserer_utfores_venter_paa_replay_naar_den_mangler() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![(sak_client_reference, sak_skuffen_id)];
    execution_repo.data.lock().unwrap().registrering_resultat =
        Some(EksekveringsregistreringResultat::EksisterteUtenVenterPublisert);

    let service = build_service(
        execution_repo.clone(),
        entity_repo,
        id_mapping_repo,
        status_publisher.clone(),
    );

    let envelope = make_opprett_sak_command(sak_client_reference);
    service.handle(&envelope).await.unwrap();

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage_status, CommandStageStatus::Venter);
    drop(events);

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(exec_data.markerte_utfores_venter, vec![envelope.command_id]);
}

#[tokio::test]
async fn markerer_ikke_utfores_venter_naar_publisering_feiler() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![(sak_client_reference, sak_skuffen_id)];
    *status_publisher.fail_next.lock().unwrap() = true;

    let service = build_service(
        execution_repo.clone(),
        entity_repo,
        id_mapping_repo,
        status_publisher.clone(),
    );

    let envelope = make_opprett_sak_command(sak_client_reference);
    let err = service.handle(&envelope).await.unwrap_err();

    assert!(err.to_string().contains("publish failed"));
    assert!(status_publisher.events.lock().unwrap().is_empty());
    let exec_data = execution_repo.data.lock().unwrap();
    assert!(exec_data.markerte_utfores_venter.is_empty());
}

#[tokio::test]
async fn registrerer_feil_ved_tilstandsfeil() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_skuffen_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference = vec![
        (sak_client_reference, sak_skuffen_id),
        (journalpost_client_reference, journalpost_skuffen_id),
    ];

    // Sak Opprettet, men journalpost har et permanent-feilet dokument.
    // neste_handling returnerer Err(Blocked), som evaluer_klarhet mapper til BlokkertVenter.
    entity_repo.data.lock().unwrap().sak_med_barn.insert(
        sak_skuffen_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_skuffen_id),
            tilstand: SakTilstand::Opprettet,
            oensket_tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: Some("2026/10".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![JournalpostMedDokumenter {
                journalpost_id: SkuffenJournalpostId::from(journalpost_skuffen_id),
                tilstand: JournalpostTilstand::DokumenterUnderArbeid,
                oensket_tilstand: JournalpostTilstand::Journalfoert,
                sikri_id: Some(200),
                journalpostnummer: Some(42),
                journalposttype: JournalpostType::InterntNotat,
                med_utsending: false,
                dokumenter: vec![DokumentMedTilstand {
                    dokument_id: SkuffenDokumentId::from(Uuid::new_v4()),
                    tilstand: DokumentTilstand::FeiletPermanent,
                    kilde: DokumentKildeTilstand::Bytes,
                }],
            }],
        },
    );

    let service = build_service(
        execution_repo.clone(),
        entity_repo,
        id_mapping_repo,
        status_publisher.clone(),
    );

    let envelope = make_journalpost_command(
        journalpost_client_reference,
        SakKey::ClientReference(sak_client_reference),
    );
    service.handle(&envelope).await.unwrap();

    // FeiletPermanent dokument → neste_handling returns Err(Irrecoverable)
    // → evaluer_klarhet maps to Feil → terminal error event published
    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(
            envelope.command_id,
            EksekveringStatus::Feil,
            Some("Tilstandsfeil ved registrering".to_string())
        )]
    );
    drop(exec_data);

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage_status, CommandStageStatus::Error);
    assert!(events[0].terminal);
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

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
                    form: Dokumentform::Bytes {
                        filtype: "PDF".to_string(),
                        dokument_referanse: Uuid::new_v4(),
                    },
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
