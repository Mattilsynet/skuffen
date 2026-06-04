use async_trait::async_trait;
use domain::eksekvering::execution::EksekveringStatus;
use domain::eksekvering::html_template::TemplateFelt;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::JournalpostType;
use domain::eksekvering::tilstand::{
    DokumentKildeTilstand, DokumentMedTilstand, DokumentTilstand, JournalpostMedDokumenter,
    JournalpostTilstand, SakMedBarn, SakTilstand, Saksansvarlig,
};
use domain::eksekvering::typer::{
    CommandLifecycleContext, CommandLifecycleEvent, CommandStage, CommandStageStatus,
};
use lib_schemas::skuffen::command::commands::{
    Command as WireCommand, CommandEnvelope as WireCommandEnvelope,
};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak, SettSaksansvarlig};
use lib_schemas::skuffen::dokument::{Dokument, Dokumentform};
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
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::services::registrer_i_eksekveringssystem::RegistrerIEksekveringssystemService;
use crate::command::{
    Command as ApplicationCommand, CommandEnvelope as ApplicationCommandEnvelope,
};

// ---------------------------------------------------------------------------
// FakeEntityTilstandRepository
// ---------------------------------------------------------------------------

type OpprettetSakRecord = (Uuid, Uuid);
type OppdatertSakRecord = (Uuid, SakTilstand, Option<i64>, Option<String>);
type OpprettetDokumentRecord = (
    Uuid,
    Uuid,
    DokumentTilstand,
    Option<Uuid>,
    Vec<TemplateFelt>,
    Uuid,
);
type OppdatertSaksansvarligRecord = (Uuid, String, String);

#[derive(Clone, Debug, PartialEq, Eq)]
enum FakeEntityEvent {
    OpprettSakTilstand {
        sak_id: Uuid,
    },
    OppdaterSakTilstand {
        sak_id: Uuid,
        tilstand: SakTilstand,
        saksnummer: Option<String>,
    },
    OpprettJournalpostTilstand {
        journalpost_id: Uuid,
        sak_id: Uuid,
    },
    OppdaterOensketSaksansvarlig {
        sak_id: Uuid,
    },
    EnsureArkivIdSakSeeded {
        sak_id: Uuid,
        saksnummer: String,
    },
}

#[derive(Default)]
struct FakeEntityTilstandData {
    opprettede_saker: Vec<OpprettetSakRecord>,
    oppdaterte_saker: Vec<OppdatertSakRecord>,
    oppdaterte_oenskede_saksansvarlige: Vec<OppdatertSaksansvarligRecord>,
    opprettede_journalposter: Vec<(Uuid, Uuid, JournalpostType, bool, Uuid)>,
    opprettede_dokumenter: Vec<OpprettetDokumentRecord>,
    sak_med_barn: HashMap<Uuid, SakMedBarn>,
    entity_events: Vec<FakeEntityEvent>,
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
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let sak_id = Uuid::from(sak_id);
        data.opprettede_saker.push((sak_id, command_id));
        data.entity_events
            .push(FakeEntityEvent::OpprettSakTilstand { sak_id });
        Ok(())
    }

    async fn oppdater_sak_tilstand(
        &self,
        sak_id: SkuffenSakId,
        tilstand: SakTilstand,
        sikri_id: Option<i64>,
        saksnummer: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let sak_id_uuid = Uuid::from(sak_id);
        let saksnummer = saksnummer.map(str::to_string);
        data.oppdaterte_saker
            .push((sak_id_uuid, tilstand, sikri_id, saksnummer.clone()));
        data.entity_events
            .push(FakeEntityEvent::OppdaterSakTilstand {
                sak_id: sak_id_uuid,
                tilstand,
                saksnummer: saksnummer.clone(),
            });

        match data.sak_med_barn.get_mut(&sak_id_uuid) {
            Some(sak) => {
                sak.tilstand = tilstand;
                sak.sikri_id = sikri_id;
                sak.saksnummer = saksnummer;
            }
            None => {
                data.sak_med_barn.insert(
                    sak_id_uuid,
                    SakMedBarn {
                        sak_id,
                        tilstand,
                        sikri_id,
                        saksnummer,
                        oensket_saksansvarlig: None,
                        naavaerende_saksansvarlig: None,
                        journalposter: Vec::new(),
                    },
                );
            }
        }
        Ok(())
    }

    async fn ensure_sak_tilstand_for_arkiv_id(
        &self,
        sak_id: SkuffenSakId,
        saksnummer: &str,
        _command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let sak_id_uuid = Uuid::from(sak_id);

        if data.sak_med_barn.contains_key(&sak_id_uuid) {
            return Ok(());
        }

        let saksnummer = saksnummer.to_string();
        data.oppdaterte_saker.push((
            sak_id_uuid,
            SakTilstand::Opprettet,
            None,
            Some(saksnummer.clone()),
        ));
        data.entity_events
            .push(FakeEntityEvent::EnsureArkivIdSakSeeded {
                sak_id: sak_id_uuid,
                saksnummer: saksnummer.clone(),
            });
        data.sak_med_barn.insert(
            sak_id_uuid,
            SakMedBarn {
                sak_id,
                tilstand: SakTilstand::Opprettet,
                sikri_id: None,
                saksnummer: Some(saksnummer),
                oensket_saksansvarlig: None,
                naavaerende_saksansvarlig: None,
                journalposter: Vec::new(),
            },
        );

        Ok(())
    }

    async fn oppdater_oensket_saksansvarlig(
        &self,
        sak_id: SkuffenSakId,
        saksbehandler_id: &str,
        saksbehandler_enhet: &str,
    ) -> Result<(), anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let sak_id_uuid = Uuid::from(sak_id);
        data.oppdaterte_oenskede_saksansvarlige.push((
            sak_id_uuid,
            saksbehandler_id.to_string(),
            saksbehandler_enhet.to_string(),
        ));
        data.entity_events
            .push(FakeEntityEvent::OppdaterOensketSaksansvarlig {
                sak_id: sak_id_uuid,
            });
        if let Some(sak) = data.sak_med_barn.get_mut(&sak_id_uuid) {
            sak.oensket_saksansvarlig = Some(Saksansvarlig {
                saksbehandler_id: saksbehandler_id.to_string(),
                enhet: saksbehandler_enhet.to_string(),
            });
        }
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
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let journalpost_id_uuid = Uuid::from(journalpost_id);
        let sak_id_uuid = Uuid::from(sak_id);
        data.opprettede_journalposter.push((
            journalpost_id_uuid,
            sak_id_uuid,
            journalposttype,
            med_utsending,
            command_id,
        ));
        data.entity_events
            .push(FakeEntityEvent::OpprettJournalpostTilstand {
                journalpost_id: journalpost_id_uuid,
                sak_id: sak_id_uuid,
            });
        if let Some(sak) = data.sak_med_barn.get_mut(&sak_id_uuid) {
            if !sak
                .journalposter
                .iter()
                .any(|journalpost| journalpost.journalpost_id == journalpost_id)
            {
                sak.journalposter.push(JournalpostMedDokumenter {
                    journalpost_id,
                    tilstand: JournalpostTilstand::IkkeRealisert,
                    sikri_id: None,
                    journalpostnummer: None,
                    journalposttype,
                    med_utsending,
                    dokumenter: Vec::new(),
                });
            }
        }
        Ok(())
    }

    async fn oppdater_journalpost_tilstand(
        &self,
        _journalpost_id: SkuffenJournalpostId,
        _tilstand: JournalpostTilstand,
        _sikri_id: Option<i64>,
        _journalpostnummer: Option<i32>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn hent_sak_id_fra_journalpost_id(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error> {
        let journalpost_id = Uuid::from(journalpost_id);
        Ok(self
            .data
            .lock()
            .unwrap()
            .sak_med_barn
            .values()
            .find(|sak| {
                sak.journalposter
                    .iter()
                    .any(|journalpost| journalpost.journalpost_id.0 == journalpost_id)
            })
            .map(|sak| sak.sak_id))
    }

    async fn hent_journalpost_id_fra_dokument_id(
        &self,
        dokument_id: SkuffenDokumentId,
    ) -> Result<Option<SkuffenJournalpostId>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .sak_med_barn
            .values()
            .flat_map(|sak| &sak.journalposter)
            .find(|journalpost| {
                journalpost
                    .dokumenter
                    .iter()
                    .any(|dokument| dokument.dokument_id == dokument_id)
            })
            .map(|journalpost| journalpost.journalpost_id))
    }

    async fn opprett_dokument_tilstand(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
        tilstand: DokumentTilstand,
        mal_referanse: Option<Uuid>,
        felter: Vec<TemplateFelt>,
        command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        let dokument_id_uuid = Uuid::from(dokument_id);
        data.opprettede_dokumenter.push((
            dokument_id_uuid,
            Uuid::from(journalpost_id),
            tilstand,
            mal_referanse,
            felter.clone(),
            command_id,
        ));
        for sak in data.sak_med_barn.values_mut() {
            if let Some(journalpost) = sak
                .journalposter
                .iter_mut()
                .find(|journalpost| journalpost.journalpost_id == journalpost_id)
            {
                journalpost.dokumenter.push(DokumentMedTilstand {
                    dokument_id,
                    tilstand,
                    kilde: match mal_referanse {
                        Some(mal_referanse) => DokumentKildeTilstand::HtmlTemplate {
                            mal_referanse,
                            felter,
                            rendered_dokument_referanse: None,
                        },
                        None => DokumentKildeTilstand::Bytes,
                    },
                });
                break;
            }
        }
        Ok(())
    }

    async fn oppdater_dokument_tilstand(
        &self,
        _dokument_id: SkuffenDokumentId,
        _tilstand: DokumentTilstand,
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

    async fn marker_klar(&self, _command_id: Uuid, _attempt_no: i32) -> Result<(), anyhow::Error> {
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

    async fn oppdater_til_klar(&self, _command_id: Uuid) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn oppdater_blokkert_detail(
        &self,
        _command_id: Uuid,
        _detalj: &str,
    ) -> Result<(), anyhow::Error> {
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
        _entity_type: MappingEntityType,
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
        _envelope: &ApplicationCommandEnvelope<ApplicationCommand>,
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
    let (saved_sak_id, cmd_id) = &entity_data.opprettede_saker[0];
    assert_eq!(*saved_sak_id, sak_skuffen_id);
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
    let (jp_id, linked_sak_id, jp_type, _, cmd_id) = &entity_data.opprettede_journalposter[0];
    assert_eq!(*jp_id, journalpost_skuffen_id);
    assert_eq!(*linked_sak_id, sak_skuffen_id);
    assert_eq!(*jp_type, JournalpostType::InterntNotat);
    assert_eq!(*cmd_id, envelope.command_id);
    assert_eq!(entity_data.opprettede_dokumenter.len(), 1);
    drop(entity_data);

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(
            envelope.command_id,
            EksekveringStatus::BlokkertVenter,
            Some("blocked_reason=entity_missing trigger_category=entity_fakta_endret".to_string())
        )]
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
            sikri_id: Some(100),
            saksnummer: Some("2025/123".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![JournalpostMedDokumenter {
                journalpost_id: SkuffenJournalpostId::from(journalpost_skuffen_id),
                tilstand: JournalpostTilstand::IkkeRealisert,
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
async fn avslutt_sak_uten_opprettet_sak_registrerer_som_blokkert_venter() {
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

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(
            envelope.command_id,
            EksekveringStatus::BlokkertVenter,
            Some("blocked_reason=entity_missing trigger_category=entity_fakta_endret".to_string())
        )]
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
async fn registrerer_invalid_som_klar_og_publiserer_venter() {
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
    // Registration mapper Invalid til Klar slik at executor eier terminal Feil.
    entity_repo.data.lock().unwrap().sak_med_barn.insert(
        sak_skuffen_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_skuffen_id),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: Some("2026/10".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![JournalpostMedDokumenter {
                journalpost_id: SkuffenJournalpostId::from(journalpost_skuffen_id),
                tilstand: JournalpostTilstand::DokumenterUnderArbeid,
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

    // FeiletPermanent dokument → planlegg_neste_handling returnerer CommandStateDecision::Invalid
    // → registration mapper til Klar → utfores::venter publiseres for executor.
    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(envelope.command_id, EksekveringStatus::Klar, None)]
    );
    drop(exec_data);

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage_status, CommandStageStatus::Venter);
    assert!(!events[0].terminal);
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn make_journalpost_command(
    journalpost_id: Uuid,
    sak_key: SakKey,
) -> ApplicationCommandEnvelope<ApplicationCommand> {
    crate::command::test_support::map_wire_envelope(WireCommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: WireCommand::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
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
    })
}

fn make_opprett_sak_command(
    sak_client_reference: Uuid,
) -> ApplicationCommandEnvelope<ApplicationCommand> {
    crate::command::test_support::map_wire_envelope(WireCommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: WireCommand::OpprettSak(OpprettSak {
            client_reference: sak_client_reference,
            sakstittel: Sakstittel("Test sak".to_string()),
            ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
            arkivdel: Arkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgang: None,
        }),
    })
}

fn make_avslutt_sak_command(sak_key: SakKey) -> ApplicationCommandEnvelope<ApplicationCommand> {
    crate::command::test_support::map_wire_envelope(WireCommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: WireCommand::AvsluttSak(AvsluttSak { sak_key }),
    })
}

fn make_sett_saksansvarlig_command(
    sak_key: SakKey,
    saksbehandler_id: &str,
    saksbehandler_enhet: &str,
) -> ApplicationCommandEnvelope<ApplicationCommand> {
    crate::command::test_support::map_wire_envelope(WireCommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: WireCommand::SettSaksansvarlig(SettSaksansvarlig {
            sak_key,
            saksbehandler_id: saksbehandler_id.to_string(),
            saksbehandler_enhet: saksbehandler_enhet.to_string(),
        }),
    })
}

// ---------------------------------------------------------------------------
// ArkivId first-seen Sak seeding behavior
// ---------------------------------------------------------------------------

#[tokio::test]
async fn journalpost_med_arkiv_id_ukjent_lokal_sak_saeder_opprettet_med_saksnummer_og_blir_klar() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_skuffen_id = Uuid::new_v4();
    let ensured_sak_id = Uuid::new_v4();
    let saksnummer = Saksnummer::new("2025/789").unwrap();

    id_mapping_repo.data.lock().unwrap().ensured_sak_id = Some(ensured_sak_id);
    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .skuffen_id_for_client_reference =
        vec![(journalpost_client_reference, journalpost_skuffen_id)];

    let service = build_service(
        execution_repo.clone(),
        entity_repo.clone(),
        id_mapping_repo,
        status_publisher,
    );

    let envelope = make_journalpost_command(
        journalpost_client_reference,
        SakKey::ArkivId(saksnummer.clone()),
    );
    service.handle(&envelope).await.unwrap();

    let entity_data = entity_repo.data.lock().unwrap();
    assert_eq!(
        entity_data.oppdaterte_saker,
        vec![(
            ensured_sak_id,
            SakTilstand::Opprettet,
            None,
            Some(saksnummer.as_str().to_string())
        )]
    );
    let seeded_sak = entity_data.sak_med_barn.get(&ensured_sak_id).unwrap();
    assert_eq!(seeded_sak.tilstand, SakTilstand::Opprettet);
    assert_eq!(seeded_sak.saksnummer.as_deref(), Some(saksnummer.as_str()));
    assert!(matches!(
        entity_data.entity_events.as_slice(),
        [
            FakeEntityEvent::EnsureArkivIdSakSeeded { .. },
            FakeEntityEvent::OpprettJournalpostTilstand { .. },
            ..
        ]
    ));
    drop(entity_data);

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(envelope.command_id, EksekveringStatus::Klar, None)]
    );
}

#[tokio::test]
async fn avslutt_sak_med_arkiv_id_ukjent_lokal_sak_saeder_foer_klarhet_og_blir_klar() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let ensured_sak_id = Uuid::new_v4();
    let saksnummer = Saksnummer::new("2025/999").unwrap();

    id_mapping_repo.data.lock().unwrap().ensured_sak_id = Some(ensured_sak_id);

    let service = build_service(
        execution_repo.clone(),
        entity_repo.clone(),
        id_mapping_repo,
        status_publisher,
    );

    let envelope = make_avslutt_sak_command(SakKey::ArkivId(saksnummer.clone()));
    service.handle(&envelope).await.unwrap();

    let entity_data = entity_repo.data.lock().unwrap();
    assert_eq!(
        entity_data.oppdaterte_saker,
        vec![(
            ensured_sak_id,
            SakTilstand::Opprettet,
            None,
            Some(saksnummer.as_str().to_string())
        )]
    );
    let seeded_sak = entity_data.sak_med_barn.get(&ensured_sak_id).unwrap();
    assert_eq!(seeded_sak.tilstand, SakTilstand::Opprettet);
    assert_eq!(seeded_sak.saksnummer.as_deref(), Some(saksnummer.as_str()));
    assert!(seeded_sak.journalposter.is_empty());
    drop(entity_data);

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(envelope.command_id, EksekveringStatus::Klar, None)]
    );
}

#[tokio::test]
async fn registrerer_done_som_klar_og_publiserer_venter() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let ensured_sak_id = Uuid::new_v4();
    let saksnummer = Saksnummer::new("2025/1000").unwrap();

    id_mapping_repo.data.lock().unwrap().ensured_sak_id = Some(ensured_sak_id);
    entity_repo.data.lock().unwrap().sak_med_barn.insert(
        ensured_sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(ensured_sak_id),
            tilstand: SakTilstand::Avsluttet,
            sikri_id: Some(100),
            saksnummer: Some(saksnummer.as_str().to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: Vec::new(),
        },
    );

    let service = build_service(
        execution_repo.clone(),
        entity_repo,
        id_mapping_repo,
        status_publisher.clone(),
    );

    let envelope = make_avslutt_sak_command(SakKey::ArkivId(saksnummer));
    service.handle(&envelope).await.unwrap();

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(envelope.command_id, EksekveringStatus::Klar, None)]
    );
    assert_eq!(exec_data.markerte_utfores_venter, vec![envelope.command_id]);
    drop(exec_data);

    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stage_status, CommandStageStatus::Venter);
    assert!(!events[0].terminal);
}

#[tokio::test]
async fn sett_saksansvarlig_med_arkiv_id_ukjent_lokal_sak_saeder_foer_oensket_update_og_blir_klar()
{
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let ensured_sak_id = Uuid::new_v4();
    let saksnummer = Saksnummer::new("2025/555").unwrap();

    id_mapping_repo.data.lock().unwrap().ensured_sak_id = Some(ensured_sak_id);

    let service = build_service(
        execution_repo.clone(),
        entity_repo.clone(),
        id_mapping_repo,
        status_publisher,
    );

    let envelope =
        make_sett_saksansvarlig_command(SakKey::ArkivId(saksnummer.clone()), "Z99999", "99");
    service.handle(&envelope).await.unwrap();

    let entity_data = entity_repo.data.lock().unwrap();
    assert_eq!(
        entity_data.oppdaterte_saker,
        vec![(
            ensured_sak_id,
            SakTilstand::Opprettet,
            None,
            Some(saksnummer.as_str().to_string())
        )]
    );
    assert_eq!(
        entity_data.oppdaterte_oenskede_saksansvarlige,
        vec![(ensured_sak_id, "Z99999".to_string(), "99".to_string())]
    );
    assert!(matches!(
        entity_data.entity_events.as_slice(),
        [
            FakeEntityEvent::EnsureArkivIdSakSeeded { .. },
            FakeEntityEvent::OppdaterOensketSaksansvarlig { .. },
            ..
        ]
    ));
    let seeded_sak = entity_data.sak_med_barn.get(&ensured_sak_id).unwrap();
    assert_eq!(seeded_sak.saksnummer.as_deref(), Some(saksnummer.as_str()));
    assert_eq!(
        seeded_sak.oensket_saksansvarlig.as_ref(),
        Some(&Saksansvarlig {
            saksbehandler_id: "Z99999".to_string(),
            enhet: "99".to_string(),
        })
    );
    drop(entity_data);

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(envelope.command_id, EksekveringStatus::Klar, None)]
    );
}

#[tokio::test]
async fn arkiv_id_saeding_overskriver_ikke_eksisterende_sak_tilstand() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();

    let ensured_sak_id = Uuid::new_v4();
    id_mapping_repo.data.lock().unwrap().ensured_sak_id = Some(ensured_sak_id);
    entity_repo.data.lock().unwrap().sak_med_barn.insert(
        ensured_sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(ensured_sak_id),
            tilstand: SakTilstand::Avsluttet,
            sikri_id: Some(100),
            saksnummer: Some("2020/111".to_string()),
            oensket_saksansvarlig: Some(Saksansvarlig {
                saksbehandler_id: "ZOLD".to_string(),
                enhet: "01".to_string(),
            }),
            naavaerende_saksansvarlig: Some(Saksansvarlig {
                saksbehandler_id: "ZOLD".to_string(),
                enhet: "01".to_string(),
            }),
            journalposter: Vec::new(),
        },
    );

    let service = build_service(
        execution_repo,
        entity_repo.clone(),
        id_mapping_repo,
        status_publisher,
    );

    let envelope = make_avslutt_sak_command(SakKey::ArkivId(Saksnummer::new("2025/111").unwrap()));
    service.handle(&envelope).await.unwrap();

    let entity_data = entity_repo.data.lock().unwrap();
    assert!(entity_data.oppdaterte_saker.is_empty());
    let existing_sak = entity_data.sak_med_barn.get(&ensured_sak_id).unwrap();
    assert_eq!(existing_sak.tilstand, SakTilstand::Avsluttet);
    assert_eq!(existing_sak.saksnummer.as_deref(), Some("2020/111"));
    assert_eq!(
        existing_sak.oensket_saksansvarlig.as_ref(),
        Some(&Saksansvarlig {
            saksbehandler_id: "ZOLD".to_string(),
            enhet: "01".to_string(),
        })
    );
}

#[tokio::test]
async fn journalpost_med_client_reference_uten_lokal_sak_saeder_ikke_arkiv_id_og_forblir_blokkert()
{
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
        status_publisher,
    );

    let envelope = make_journalpost_command(
        journalpost_client_reference,
        SakKey::ClientReference(sak_client_reference),
    );
    service.handle(&envelope).await.unwrap();

    let entity_data = entity_repo.data.lock().unwrap();
    assert!(entity_data.oppdaterte_saker.is_empty());
    drop(entity_data);

    let exec_data = execution_repo.data.lock().unwrap();
    assert_eq!(
        exec_data.opprettede_eksekveringer,
        vec![(
            envelope.command_id,
            EksekveringStatus::BlokkertVenter,
            Some("blocked_reason=entity_missing trigger_category=entity_fakta_endret".to_string())
        )]
    );
}
