use async_trait::async_trait;
use domain::eksekvering::html_template::TemplateFelt;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::JournalpostType;
use domain::eksekvering::tilstand::{
    DokumentKildeTilstand, DokumentMedTilstand, DokumentTilstand, JournalpostMedDokumenter,
    JournalpostTilstand, SakMedBarn, SakTilstand,
};
use lib_schemas::skuffen::command::commands::{
    Command as WireCommand, CommandEnvelope as WireCommandEnvelope,
};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak};
use lib_schemas::skuffen::dokument::Dokument;
use lib_schemas::skuffen::dokument::Dokumentform;
use lib_schemas::skuffen::query::queries::SakKey;
use lib_schemas::skuffen::sak::{Ordningsverdi, Sakstittel};
use lib_schemas::skuffen::tilgang::Tilgjengelighet;
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
use crate::command::ports::entity_tilstand_port::EntityTilstandRepository;
use crate::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::services::reevaluer_ventende_kommandoer::ReevaluerVentendeKommandoerService;
use crate::command::{
    Command as ApplicationCommand, CommandEnvelope as ApplicationCommandEnvelope,
};
use domain::eksekvering::typer::{CommandLifecycleContext, CommandLifecycleEvent};

// ---------------------------------------------------------------------------
// FakeEntityTilstandRepository
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct FakeEntityTilstandRepository {
    sak_med_barn: Arc<Mutex<HashMap<Uuid, SakMedBarn>>>,
}

#[async_trait]
impl EntityTilstandRepository for FakeEntityTilstandRepository {
    async fn opprett_sak_tilstand(
        &self,
        _sak_id: SkuffenSakId,
        _command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
    async fn oppdater_sak_tilstand(
        &self,
        _sak_id: SkuffenSakId,
        _tilstand: SakTilstand,
        _sikri_id: Option<i64>,
        _saksnummer: Option<&str>,
    ) -> Result<(), anyhow::Error> {
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
    async fn ensure_sak_tilstand_for_arkiv_id(
        &self,
        _sak_id: SkuffenSakId,
        _saksnummer: &str,
        _command_id: Uuid,
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
        _journalpost_id: SkuffenJournalpostId,
        _sak_id: SkuffenSakId,
        _journalposttype: JournalpostType,
        _med_utsending: bool,
        _command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
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
            .sak_med_barn
            .lock()
            .unwrap()
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
            .sak_med_barn
            .lock()
            .unwrap()
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
        _dokument_id: SkuffenDokumentId,
        _journalpost_id: SkuffenJournalpostId,
        _tilstand: DokumentTilstand,
        _mal_referanse: Option<Uuid>,
        _felter: Vec<TemplateFelt>,
        _command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
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
            .sak_med_barn
            .lock()
            .unwrap()
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
    blokkert_venter_for_sak: HashMap<Uuid, Vec<EksekveringKommando>>,
    oppdatert_til_klar: Vec<Uuid>,
    oppdatert_blokkert_detail: Vec<(Uuid, String)>,
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
struct FakeDonePublisher;

#[async_trait]
impl EksekveringKvitteringPublisher for FakeDonePublisher {
    async fn publiser_done(
        &self,
        _command: &ApplicationCommandEnvelope<ApplicationCommand>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeStatusProjector;

#[async_trait]
impl CommandOutwardStatusProjector for FakeStatusProjector {
    async fn resolve_context(
        &self,
        _envelope: &ApplicationCommandEnvelope<ApplicationCommand>,
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
        sak_id: SkuffenSakId,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error> {
        Ok(self
            .data
            .lock()
            .unwrap()
            .blokkert_venter_for_sak
            .get(&Uuid::from(sak_id))
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

    async fn oppdater_blokkert_detail(
        &self,
        command_id: Uuid,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .oppdatert_blokkert_detail
            .push((command_id, detalj.to_string()));
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

// ---------------------------------------------------------------------------
// FakeIdMappingRepository
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn etter_sak_endret_gjor_journalpostkommando_klar_nar_saksnummer_kommer() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_tilstand_repo = FakeEntityTilstandRepository::default();
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

    // Sak is Opprettet with saksnummer, journalpost is IkkeRealisert
    // → planlegg_neste_handling returnerer Ready(OpprettJournalpost) → command blir Klar
    entity_tilstand_repo.sak_med_barn.lock().unwrap().insert(
        sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_id),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: Some("2026/1".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![JournalpostMedDokumenter {
                journalpost_id: SkuffenJournalpostId::from(journalpost_id),
                tilstand: JournalpostTilstand::IkkeRealisert,
                sikri_id: None,
                journalpostnummer: None,
                journalposttype: JournalpostType::InterntNotat,
                med_utsending: false,
                dokumenter: vec![DokumentMedTilstand {
                    dokument_id: SkuffenDokumentId::from(dokument_id),
                    tilstand: DokumentTilstand::IkkeRealisert,
                    kilde: DokumentKildeTilstand::Bytes,
                }],
            }],
        },
    );

    execution_repo
        .data
        .lock()
        .unwrap()
        .blokkert_venter_for_sak
        .insert(
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
        Box::new(entity_tilstand_repo),
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
    assert!(data.oppdatert_til_feil.is_empty());
}

#[tokio::test]
async fn etter_sak_endret_gjor_blokkert_command_klar_nar_den_reevaluerer_til_done() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_tilstand_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let command_id = Uuid::new_v4();

    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .client_to_skuffen
        .insert(sak_client_reference, sak_id);

    // Sak er allerede opprettet med saksnummer. planlegg_neste_handling returnerer
    // Done for OpprettSak, men wake-up flytter command til Klar slik at executor
    // kan publisere terminal success/done.
    entity_tilstand_repo.sak_med_barn.lock().unwrap().insert(
        sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_id),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: Some("2026/1".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![],
        },
    );

    execution_repo
        .data
        .lock()
        .unwrap()
        .blokkert_venter_for_sak
        .insert(
            sak_id,
            vec![EksekveringKommando {
                command_id,
                envelope: make_opprett_sak_command(sak_client_reference),
                attempt_no: 0,
                utfores_venter_publisert: true,
            }],
        );

    let service = ReevaluerVentendeKommandoerService::new(
        Box::new(execution_repo.clone()),
        Box::new(entity_tilstand_repo),
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
    assert!(data.oppdatert_til_feil.is_empty());
}

#[tokio::test]
async fn etter_journalpost_endret_gjor_avslutt_sak_klar_nar_journalpost_er_ferdig() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_tilstand_repo = FakeEntityTilstandRepository::default();
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

    entity_tilstand_repo.sak_med_barn.lock().unwrap().insert(
        sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_id),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: Some("2026/1".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![JournalpostMedDokumenter {
                journalpost_id: SkuffenJournalpostId::from(journalpost_id),
                tilstand: JournalpostTilstand::Journalfoert,
                sikri_id: Some(200),
                journalpostnummer: Some(42),
                journalposttype: JournalpostType::InterntNotat,
                med_utsending: false,
                dokumenter: vec![DokumentMedTilstand {
                    dokument_id: SkuffenDokumentId::from(Uuid::new_v4()),
                    tilstand: DokumentTilstand::Ok,
                    kilde: DokumentKildeTilstand::Bytes,
                }],
            }],
        },
    );

    execution_repo
        .data
        .lock()
        .unwrap()
        .blokkert_venter_for_sak
        .insert(
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
        Box::new(entity_tilstand_repo),
        Box::new(id_mapping_repo),
        Box::new(status_publisher),
        Box::new(done_publisher),
        Box::new(FakeStatusProjector),
    );

    service
        .etter_journalpost_endret(SkuffenJournalpostId::from(journalpost_id))
        .await
        .unwrap();

    let data = execution_repo.data.lock().unwrap();
    assert_eq!(data.oppdatert_til_klar, vec![command_id]);
    assert!(data.oppdatert_til_feil.is_empty());
}

#[tokio::test]
async fn etter_sak_endret_gjor_avslutt_sak_klar_naar_saksansvarlig_matcher() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_tilstand_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let command_id = Uuid::new_v4();

    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .client_to_skuffen
        .insert(sak_client_reference, sak_id);

    entity_tilstand_repo.sak_med_barn.lock().unwrap().insert(
        sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_id),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: Some("2026/1".to_string()),
            oensket_saksansvarlig: Some(domain::eksekvering::tilstand::Saksansvarlig {
                saksbehandler_id: "Z12345".to_string(),
                enhet: "42".to_string(),
            }),
            naavaerende_saksansvarlig: Some(domain::eksekvering::tilstand::Saksansvarlig {
                saksbehandler_id: "Z12345".to_string(),
                enhet: "42".to_string(),
            }),
            journalposter: vec![],
        },
    );

    execution_repo
        .data
        .lock()
        .unwrap()
        .blokkert_venter_for_sak
        .insert(
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
        Box::new(entity_tilstand_repo),
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
    assert!(data.oppdatert_blokkert_detail.is_empty());
    assert!(data.oppdatert_til_feil.is_empty());
}

#[tokio::test]
async fn etter_sak_endret_holder_avslutt_sak_blokkert_naar_saksansvarlig_mangler() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_tilstand_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let command_id = Uuid::new_v4();

    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .client_to_skuffen
        .insert(sak_client_reference, sak_id);

    entity_tilstand_repo.sak_med_barn.lock().unwrap().insert(
        sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_id),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: Some("2026/1".to_string()),
            oensket_saksansvarlig: Some(domain::eksekvering::tilstand::Saksansvarlig {
                saksbehandler_id: "Z12345".to_string(),
                enhet: "42".to_string(),
            }),
            naavaerende_saksansvarlig: None,
            journalposter: vec![],
        },
    );

    execution_repo
        .data
        .lock()
        .unwrap()
        .blokkert_venter_for_sak
        .insert(
            sak_id,
            vec![EksekveringKommando {
                command_id,
                envelope: make_avslutt_sak_command(sak_client_reference),
                attempt_no: 0,
                utfores_venter_publisert: false,
            }],
        );

    let service = ReevaluerVentendeKommandoerService::new(
        Box::new(execution_repo.clone()),
        Box::new(entity_tilstand_repo),
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
    assert!(data.oppdatert_til_klar.is_empty());
    assert_eq!(
        data.oppdatert_blokkert_detail,
        vec![(
            command_id,
            "blocked_reason=saksansvarlig_ikke_satt trigger_category=saksansvarlig_oppdatert"
                .to_string()
        )]
    );
    assert!(data.oppdatert_til_feil.is_empty());
}

#[tokio::test]
async fn etter_journalpost_endret_noop_nar_journalpost_mangler() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_tilstand_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();

    let service = ReevaluerVentendeKommandoerService::new(
        Box::new(execution_repo.clone()),
        Box::new(entity_tilstand_repo),
        Box::new(id_mapping_repo),
        Box::new(status_publisher),
        Box::new(done_publisher),
        Box::new(FakeStatusProjector),
    );

    service
        .etter_journalpost_endret(SkuffenJournalpostId::from(Uuid::new_v4()))
        .await
        .unwrap();

    let data = execution_repo.data.lock().unwrap();
    assert!(data.oppdatert_til_klar.is_empty());
    assert!(data.oppdatert_til_feil.is_empty());
}

#[tokio::test]
async fn etter_sak_endret_gir_feil_ved_permanent_feilet_dokument() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_tilstand_repo = FakeEntityTilstandRepository::default();
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

    // Sak Opprettet, ønsker Avsluttet, men journalpost har feilet dokument permanent.
    // → planlegg_neste_handling returnerer Invalid → command blir Feil
    entity_tilstand_repo.sak_med_barn.lock().unwrap().insert(
        sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_id),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: Some("2026/1".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![JournalpostMedDokumenter {
                journalpost_id: SkuffenJournalpostId::from(journalpost_id),
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

    execution_repo
        .data
        .lock()
        .unwrap()
        .blokkert_venter_for_sak
        .insert(
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
        Box::new(entity_tilstand_repo),
        Box::new(id_mapping_repo),
        Box::new(status_publisher.clone()),
        Box::new(done_publisher.clone()),
        Box::new(FakeStatusProjector),
    );

    service
        .etter_sak_endret(SkuffenSakId::from(sak_id))
        .await
        .unwrap();

    // Invalid fra planlegg_neste_handling → command blir Feil og terminal event publiseres
    let data = execution_repo.data.lock().unwrap();
    assert!(data.oppdatert_til_klar.is_empty());
    assert_eq!(data.oppdatert_til_feil.len(), 1);
    assert_eq!(data.oppdatert_til_feil[0].0, command_id);
    // Terminal error event published
    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].terminal);
}

#[tokio::test]
async fn etter_sak_endret_oppdaterer_blocked_detail_nar_command_fortsatt_er_blokkert() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_tilstand_repo = FakeEntityTilstandRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let status_publisher = FakeStatusPublisher::default();
    let done_publisher = FakeDonePublisher::default();

    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let command_id = Uuid::new_v4();

    id_mapping_repo
        .data
        .lock()
        .unwrap()
        .client_to_skuffen
        .insert(sak_client_reference, sak_id);

    entity_tilstand_repo.sak_med_barn.lock().unwrap().insert(
        sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_id),
            tilstand: SakTilstand::IkkeRealisert,
            sikri_id: None,
            saksnummer: None,
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![],
        },
    );

    execution_repo
        .data
        .lock()
        .unwrap()
        .blokkert_venter_for_sak
        .insert(
            sak_id,
            vec![EksekveringKommando {
                command_id,
                envelope: make_avslutt_sak_command(sak_client_reference),
                attempt_no: 0,
                utfores_venter_publisert: false,
            }],
        );

    let service = ReevaluerVentendeKommandoerService::new(
        Box::new(execution_repo.clone()),
        Box::new(entity_tilstand_repo),
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
    assert!(data.oppdatert_til_klar.is_empty());
    assert_eq!(
        data.oppdatert_blokkert_detail,
        vec![(
            command_id,
            "blocked_reason=saksnummer_mangler trigger_category=sak_fakta_endret".to_string()
        )]
    );
}

#[tokio::test]
async fn etter_dokument_endret_gjor_eierkommando_klar_og_dekker_sak_wakeup() {
    let execution_repo = FakeExecutionRepository::default();
    let entity_tilstand_repo = FakeEntityTilstandRepository::default();
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

    entity_tilstand_repo.sak_med_barn.lock().unwrap().insert(
        sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_id),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: Some("2026/1".to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![JournalpostMedDokumenter {
                journalpost_id: SkuffenJournalpostId::from(journalpost_id),
                tilstand: JournalpostTilstand::Opprettet,
                sikri_id: Some(200),
                journalpostnummer: Some(42),
                journalposttype: JournalpostType::InterntNotat,
                med_utsending: false,
                dokumenter: vec![DokumentMedTilstand {
                    dokument_id: SkuffenDokumentId::from(dokument_id),
                    tilstand: DokumentTilstand::Ok,
                    kilde: DokumentKildeTilstand::Bytes,
                }],
            }],
        },
    );

    execution_repo
        .data
        .lock()
        .unwrap()
        .blokkert_venter_for_sak
        .insert(
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
        Box::new(entity_tilstand_repo),
        Box::new(id_mapping_repo),
        Box::new(status_publisher),
        Box::new(done_publisher),
        Box::new(FakeStatusProjector),
    );

    service
        .etter_dokument_endret(SkuffenDokumentId::from(dokument_id))
        .await
        .unwrap();

    let data = execution_repo.data.lock().unwrap();
    assert_eq!(data.oppdatert_til_klar, vec![command_id]);
    assert!(data.oppdatert_til_feil.is_empty());
}

fn make_internt_notat_command(
    journalpost_client_reference: Uuid,
    sak_client_reference: Uuid,
    dokument_client_reference: Uuid,
) -> ApplicationCommandEnvelope<ApplicationCommand> {
    crate::command::test_support::map_wire_envelope(WireCommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: WireCommand::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
            felles: JournalpostCommon {
                client_reference: journalpost_client_reference,
                tittel: "Internt notat".to_string(),
                dokument_dato: "2025-01-01".to_string(),
                saksbehandler: "Z12345".to_string(),
                saksbehandler_enhet: "42".to_string(),
                tilgjengelighet: Tilgjengelighet::Offentlig,
                dokumenter: vec![Dokument {
                    client_reference: dokument_client_reference,
                    tittel: "Vedlegg".to_string(),
                    form: Dokumentform::Bytes {
                        filtype: "PDF".to_string(),
                        dokument_referanse: Uuid::new_v4(),
                    },
                }],
                sak_key: SakKey::ClientReference(sak_client_reference),
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
            sakstittel: Sakstittel::try_from("Test sak".to_string()).unwrap(),
            ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
            arkivdel: Arkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgjengelighet: Tilgjengelighet::Offentlig,
        }),
    })
}

fn make_avslutt_sak_command(
    sak_client_reference: Uuid,
) -> ApplicationCommandEnvelope<ApplicationCommand> {
    crate::command::test_support::map_wire_envelope(WireCommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: WireCommand::AvsluttSak(AvsluttSak {
            sak_key: SakKey::ClientReference(sak_client_reference),
        }),
    })
}
