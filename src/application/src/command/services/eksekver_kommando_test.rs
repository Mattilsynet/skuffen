use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domain::eksekvering::html_template::TemplateFelt;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::tilstand::{
    DokumentKildeTilstand, DokumentMedTilstand, DokumentTilstand, JournalpostMedDokumenter,
    JournalpostTilstand, JournalpostType, SakMedBarn, SakTilstand,
};
use domain::eksekvering::typer::{CommandLifecycleContext, CommandLifecycleEvent};
use lib_schemas::skuffen::command::commands::{
    Command as WireCommand, CommandEnvelope as WireCommandEnvelope,
};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::dokument::{Dokument, Dokumentform};
use lib_schemas::skuffen::query::queries::SakKey;
use lib_schemas::skuffen::tilgang::Tilgjengelighet;
use uuid::Uuid;

use crate::command::ports::command_execution_port::{
    CommandExecutionRepository, EksekveringKommando, EksekveringsregistreringResultat,
    NyKommandoEksekvering,
};
use crate::command::ports::dokument_lager_port::{DokumentFil, DokumentLager};
use crate::command::ports::dokument_renderer_port::{
    DokumentRenderer, RendererFeil, RendererKontekst,
};
use crate::command::ports::eksekvering_port::{
    ArkivGateway, EksekveringKvitteringPublisher, EksekveringStatusPublisher,
    OpprettJournalpostResultat, Utsendingsvalg,
};
use crate::command::ports::entity_tilstand_port::EntityTilstandRepository;
use crate::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};
use crate::command::ports::status_projection_port::CommandOutwardStatusProjector;
use crate::command::ports::ventende_kommando_wakeup_port::VentendeKommandoWakeup;
use crate::command::services::eksekver_kommando::{EksekverKommandoService, ExecutionOutcome};
use crate::command::services::eksekvering_worker::EksekveringWorker;
use crate::command::{
    Command as ApplicationCommand, CommandEnvelope as ApplicationCommandEnvelope,
};

type OppdatertJournalpost = (Uuid, JournalpostTilstand, Option<i64>, Option<i32>);

#[derive(Clone, Default)]
struct FakeEntityTilstandRepository {
    sak_med_barn: Arc<Mutex<HashMap<Uuid, SakMedBarn>>>,
    oppdaterte_journalposter: Arc<Mutex<Vec<OppdatertJournalpost>>>,
    oppdaterte_dokumenter: Arc<Mutex<Vec<(Uuid, DokumentTilstand)>>>,
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
        sak_id: SkuffenSakId,
        tilstand: SakTilstand,
        sikri_id: Option<i64>,
        saksnummer: Option<&str>,
    ) -> Result<(), anyhow::Error> {
        if let Some(sak) = self.sak_med_barn.lock().unwrap().get_mut(&sak_id.0) {
            sak.tilstand = tilstand;
            sak.sikri_id = sikri_id;
            sak.saksnummer = saksnummer.map(ToOwned::to_owned);
        }
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
        journalpost_id: SkuffenJournalpostId,
        tilstand: JournalpostTilstand,
        sikri_id: Option<i64>,
        journalpostnummer: Option<i32>,
    ) -> Result<(), anyhow::Error> {
        self.oppdaterte_journalposter.lock().unwrap().push((
            journalpost_id.0,
            tilstand,
            sikri_id,
            journalpostnummer,
        ));
        let mut saker = self.sak_med_barn.lock().unwrap();
        for sak in saker.values_mut() {
            if let Some(jp) = sak
                .journalposter
                .iter_mut()
                .find(|jp| jp.journalpost_id == journalpost_id)
            {
                jp.tilstand = tilstand;
                jp.sikri_id = sikri_id;
                jp.journalpostnummer = journalpostnummer;
            }
        }
        Ok(())
    }

    async fn hent_sak_id_fra_journalpost_id(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error> {
        Ok(self
            .sak_med_barn
            .lock()
            .unwrap()
            .values()
            .find(|sak| {
                sak.journalposter
                    .iter()
                    .any(|jp| jp.journalpost_id == journalpost_id)
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
        dokument_id: SkuffenDokumentId,
        tilstand: DokumentTilstand,
    ) -> Result<(), anyhow::Error> {
        self.oppdaterte_dokumenter
            .lock()
            .unwrap()
            .push((dokument_id.0, tilstand));
        let mut saker = self.sak_med_barn.lock().unwrap();
        for sak in saker.values_mut() {
            for jp in &mut sak.journalposter {
                if let Some(dokument) = jp
                    .dokumenter
                    .iter_mut()
                    .find(|dokument| dokument.dokument_id == dokument_id)
                {
                    dokument.tilstand = tilstand;
                }
            }
        }
        Ok(())
    }

    async fn oppdater_rendered_dokument_referanse(
        &self,
        dokument_id: SkuffenDokumentId,
        rendered_dokument_referanse: Uuid,
    ) -> Result<(), anyhow::Error> {
        let mut saker = self.sak_med_barn.lock().unwrap();
        for sak in saker.values_mut() {
            for jp in &mut sak.journalposter {
                let Some(dokument) = jp
                    .dokumenter
                    .iter_mut()
                    .find(|dokument| dokument.dokument_id == dokument_id)
                else {
                    continue;
                };
                if let DokumentKildeTilstand::HtmlTemplate {
                    rendered_dokument_referanse: reference,
                    ..
                } = &mut dokument.kilde
                {
                    *reference = Some(rendered_dokument_referanse);
                }
            }
        }
        Ok(())
    }

    async fn hent_sak_med_barn(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<SakMedBarn>, anyhow::Error> {
        Ok(self.sak_med_barn.lock().unwrap().get(&sak_id.0).cloned())
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

#[derive(Clone, Default)]
struct FakeArkivGateway {
    opprett_journalpost_calls: Arc<Mutex<Vec<Uuid>>>,
    legg_til_vedlegg_calls: Arc<Mutex<Vec<Uuid>>>,
    journalfoer_calls: Arc<Mutex<Vec<i32>>>,
    opprett_sak_error: Option<String>,
}

#[async_trait]
impl ArkivGateway for FakeArkivGateway {
    async fn opprett_sak(
        &self,
        _command: &ApplicationCommandEnvelope<ApplicationCommand>,
    ) -> Result<String, anyhow::Error> {
        if let Some(error) = &self.opprett_sak_error {
            anyhow::bail!(error.clone());
        }
        Ok("2026/1".to_string())
    }

    async fn opprett_journalpost(
        &self,
        command: &ApplicationCommandEnvelope<ApplicationCommand>,
        _journalpost: &JournalpostMedDokumenter,
        _saksnummer: &str,
        _utsending: Option<Utsendingsvalg>,
    ) -> Result<OpprettJournalpostResultat, anyhow::Error> {
        self.opprett_journalpost_calls
            .lock()
            .unwrap()
            .push(command.command_id);
        Ok(OpprettJournalpostResultat { journalpost_id: 42 })
    }

    async fn legg_til_vedlegg(
        &self,
        _command: &ApplicationCommandEnvelope<ApplicationCommand>,
        _journalpost_id: i32,
        _dokument_ids: Vec<Uuid>,
    ) -> Result<Vec<Option<i32>>, anyhow::Error> {
        self.legg_til_vedlegg_calls
            .lock()
            .unwrap()
            .push(_command.command_id);
        Ok(vec![Some(1)])
    }

    async fn sett_journalpost_status(
        &self,
        journalpost_id: i32,
        _status: &str,
    ) -> Result<(), anyhow::Error> {
        self.journalfoer_calls.lock().unwrap().push(journalpost_id);
        Ok(())
    }

    async fn avskriv_journalpost(
        &self,
        _journalpost_id: i32,
        _avskrivingsmaate: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn avslutt_sak(&self, _saksnummer: &str) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn sett_saksansvarlig(
        &self,
        _saksnummer: &str,
        _saksbehandler: &str,
        _saksbehandler_enhet: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[derive(Default)]
struct FakeExecutionData {
    next_command: Option<EksekveringKommando>,
    markert_kjorer: Vec<Uuid>,
    markert_klar: Vec<Uuid>,
    markert_ok: Vec<Uuid>,
    markert_blokkert: Vec<(Uuid, String)>,
    markert_retry: Vec<(Uuid, String)>,
    markert_feil: Vec<(Uuid, String)>,
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
        Ok(EksekveringsregistreringResultat::Nyregistrert)
    }

    async fn marker_utfores_venter_publisert(
        &self,
        _command_id: Uuid,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn hent_neste_kjorbare(&self) -> Result<Option<EksekveringKommando>, anyhow::Error> {
        Ok(self.data.lock().unwrap().next_command.take())
    }

    async fn marker_kjorer(&self, command_id: Uuid) -> Result<i32, anyhow::Error> {
        self.data.lock().unwrap().markert_kjorer.push(command_id);
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

    async fn marker_klar(&self, command_id: Uuid, _attempt_no: i32) -> Result<(), anyhow::Error> {
        self.oppdater_til_klar(command_id).await
    }

    async fn marker_ok(&self, command_id: Uuid, _attempt_no: i32) -> Result<(), anyhow::Error> {
        self.data.lock().unwrap().markert_ok.push(command_id);
        Ok(())
    }

    async fn marker_retry_venter(
        &self,
        command_id: Uuid,
        _attempt_no: i32,
        detalj: &str,
        _retry_ready_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .markert_retry
            .push((command_id, detalj.to_string()));
        Ok(())
    }

    async fn marker_blokkert_venter(
        &self,
        command_id: Uuid,
        _attempt_no: i32,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .markert_blokkert
            .push((command_id, detalj.to_string()));
        Ok(())
    }

    async fn marker_feil(
        &self,
        command_id: Uuid,
        _attempt_no: i32,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .markert_feil
            .push((command_id, detalj.to_string()));
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

    async fn oppdater_til_klar(&self, command_id: Uuid) -> Result<(), anyhow::Error> {
        self.data.lock().unwrap().markert_klar.push(command_id);
        Ok(())
    }

    async fn oppdater_blokkert_detail(
        &self,
        _command_id: Uuid,
        _detalj: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn oppdater_til_feil(&self, command_id: Uuid, detalj: &str) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .markert_feil
            .push((command_id, detalj.to_string()));
        Ok(())
    }

    async fn reset_kjorer_til_klar(&self) -> Result<u64, anyhow::Error> {
        Ok(0)
    }
}

#[derive(Clone, Default)]
struct FakeIdMappingRepository {
    sak_mapping: Arc<Mutex<HashMap<Uuid, Uuid>>>,
    journalpost_mapping: Arc<Mutex<HashMap<Uuid, Uuid>>>,
    dokument_mapping: Arc<Mutex<HashMap<Uuid, Uuid>>>,
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
            .sak_mapping
            .lock()
            .unwrap()
            .get(&client_reference)
            .copied()
            .map(SkuffenSakId::from))
    }

    async fn hent_journalpost_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenJournalpostId>, anyhow::Error> {
        Ok(self
            .journalpost_mapping
            .lock()
            .unwrap()
            .get(&client_reference)
            .copied()
            .map(SkuffenJournalpostId::from))
    }

    async fn hent_dokument_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenDokumentId>, anyhow::Error> {
        Ok(self
            .dokument_mapping
            .lock()
            .unwrap()
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
        Ok(SkuffenSakId::from(Uuid::new_v4()))
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
struct FakeWakeup {
    sak_endret: Arc<Mutex<Vec<Uuid>>>,
    journalpost_endret: Arc<Mutex<Vec<Uuid>>>,
    dokument_endret: Arc<Mutex<Vec<Uuid>>>,
}

#[async_trait]
impl VentendeKommandoWakeup for FakeWakeup {
    async fn etter_sak_endret(&self, sak_id: SkuffenSakId) -> Result<(), anyhow::Error> {
        self.sak_endret.lock().unwrap().push(sak_id.0);
        Ok(())
    }

    async fn etter_journalpost_endret(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<(), anyhow::Error> {
        self.journalpost_endret
            .lock()
            .unwrap()
            .push(journalpost_id.0);
        Ok(())
    }

    async fn etter_dokument_endret(
        &self,
        dokument_id: SkuffenDokumentId,
    ) -> Result<(), anyhow::Error> {
        self.dokument_endret.lock().unwrap().push(dokument_id.0);
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeDokumentRenderer;

#[async_trait]
impl DokumentRenderer for FakeDokumentRenderer {
    async fn render(
        &self,
        _html: &[u8],
        _kontekst: RendererKontekst,
    ) -> Result<Vec<u8>, RendererFeil> {
        Ok(Vec::new())
    }
}

#[derive(Clone, Default)]
struct PanickingDokumentRenderer;

#[async_trait]
impl DokumentRenderer for PanickingDokumentRenderer {
    async fn render(
        &self,
        _html: &[u8],
        _kontekst: RendererKontekst,
    ) -> Result<Vec<u8>, RendererFeil> {
        panic!("renderer skal ikke kalles når rendered_dokument_referanse finnes")
    }
}

#[derive(Clone, Default)]
struct FakeDokumentLager {
    files: Arc<Mutex<HashMap<Uuid, DokumentFil>>>,
}

impl FakeDokumentLager {
    fn with_file(file: DokumentFil) -> Self {
        let files = Arc::new(Mutex::new(HashMap::new()));
        files.lock().unwrap().insert(file.id, file);
        Self { files }
    }
}

#[async_trait]
impl DokumentLager for FakeDokumentLager {
    async fn save(&self, file: DokumentFil) -> Result<(), anyhow::Error> {
        self.files.lock().unwrap().insert(file.id, file);
        Ok(())
    }

    async fn get(&self, id: Uuid) -> Result<Option<DokumentFil>, anyhow::Error> {
        Ok(self.files.lock().unwrap().get(&id).cloned())
    }
}

#[tokio::test]
async fn handle_utfoerer_akkurat_en_operasjon_og_returnerer_klar_nar_mer_arbeid_gjenstaar() {
    let command_id = Uuid::new_v4();
    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let dokument_id = Uuid::new_v4();
    let envelope = make_internt_notat_command(
        command_id,
        journalpost_client_reference,
        sak_client_reference,
    );

    let entity_repo = FakeEntityTilstandRepository::default();
    entity_repo.sak_med_barn.lock().unwrap().insert(
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

    let arkiv_gateway = FakeArkivGateway::default();
    let id_mapping = FakeIdMappingRepository::default();
    id_mapping
        .sak_mapping
        .lock()
        .unwrap()
        .insert(sak_client_reference, sak_id);
    id_mapping
        .journalpost_mapping
        .lock()
        .unwrap()
        .insert(journalpost_client_reference, journalpost_id);

    let service = build_executor(
        entity_repo.clone(),
        arkiv_gateway.clone(),
        id_mapping,
        FakeWakeup::default(),
    );

    let outcome = service.handle(envelope, 1).await.unwrap();

    assert_eq!(outcome, ExecutionOutcome::Klar);
    assert_eq!(
        arkiv_gateway
            .opprett_journalpost_calls
            .lock()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        entity_repo.oppdaterte_journalposter.lock().unwrap().len(),
        1
    );
    assert!(
        arkiv_gateway
            .legg_til_vedlegg_calls
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(arkiv_gateway.journalfoer_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn html_template_mangler_mal_retryer_uten_opprett_journalpost() {
    let command_id = Uuid::new_v4();
    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let dokument_id = Uuid::new_v4();
    let mal_referanse = Uuid::new_v4();
    let envelope = make_internt_notat_command(
        command_id,
        journalpost_client_reference,
        sak_client_reference,
    );

    let entity_repo = FakeEntityTilstandRepository::default();
    entity_repo.sak_med_barn.lock().unwrap().insert(
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
                    tilstand: DokumentTilstand::AvventerRendring,
                    kilde: DokumentKildeTilstand::HtmlTemplate {
                        mal_referanse,
                        felter: vec![TemplateFelt::Saksnummer],
                        rendered_dokument_referanse: None,
                    },
                }],
            }],
        },
    );

    let arkiv_gateway = FakeArkivGateway::default();
    let id_mapping = FakeIdMappingRepository::default();
    id_mapping
        .sak_mapping
        .lock()
        .unwrap()
        .insert(sak_client_reference, sak_id);
    id_mapping
        .journalpost_mapping
        .lock()
        .unwrap()
        .insert(journalpost_client_reference, journalpost_id);

    let service = build_executor(
        entity_repo.clone(),
        arkiv_gateway.clone(),
        id_mapping,
        FakeWakeup::default(),
    );

    let outcome = service.handle(envelope, 1).await.unwrap();

    assert!(matches!(
        outcome,
        ExecutionOutcome::Retrying { last_error: Some(ref detail) }
            if detail.starts_with("render_html_mal_mangler")
    ));
    assert!(
        arkiv_gateway
            .opprett_journalpost_calls
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(entity_repo.oppdaterte_dokumenter.lock().unwrap().is_empty());
    let sak = entity_repo
        .sak_med_barn
        .lock()
        .unwrap()
        .get(&sak_id)
        .cloned()
        .unwrap();
    assert_eq!(
        sak.journalposter[0].dokumenter[0].tilstand,
        DokumentTilstand::AvventerRendring
    );
}

#[tokio::test]
async fn html_template_med_saksnummer_felt_blokkerer_uten_saksnummer() {
    let command_id = Uuid::new_v4();
    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let dokument_id = Uuid::new_v4();
    let mal_referanse = Uuid::new_v4();
    let envelope = make_internt_notat_command(
        command_id,
        journalpost_client_reference,
        sak_client_reference,
    );

    let entity_repo = FakeEntityTilstandRepository::default();
    entity_repo.sak_med_barn.lock().unwrap().insert(
        sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_id),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: None,
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
                    tilstand: DokumentTilstand::AvventerRendring,
                    kilde: DokumentKildeTilstand::HtmlTemplate {
                        mal_referanse,
                        felter: vec![TemplateFelt::Saksnummer],
                        rendered_dokument_referanse: None,
                    },
                }],
            }],
        },
    );

    let arkiv_gateway = FakeArkivGateway::default();
    let id_mapping = FakeIdMappingRepository::default();
    id_mapping
        .sak_mapping
        .lock()
        .unwrap()
        .insert(sak_client_reference, sak_id);
    id_mapping
        .journalpost_mapping
        .lock()
        .unwrap()
        .insert(journalpost_client_reference, journalpost_id);
    let status_publisher = FakeStatusPublisher::default();

    let service = EksekverKommandoService::new(
        Box::new(entity_repo.clone()),
        Box::new(arkiv_gateway.clone()),
        Box::new(PanickingDokumentRenderer),
        Box::new(FakeDokumentLager::with_file(DokumentFil {
            id: mal_referanse,
            data: b"<html>{{saksnummer}}</html>".to_vec(),
            filename: Some("template.html".to_string()),
            content_type: Some("text/html".to_string()),
            metadata: Default::default(),
        })),
        Box::new(status_publisher.clone()),
        Box::new(FakeDonePublisher),
        Box::new(id_mapping),
        Box::new(FakeStatusProjector),
        Box::new(FakeWakeup::default()),
    );

    let outcome = service.handle(envelope, 1).await.unwrap();

    assert!(matches!(
        outcome,
        ExecutionOutcome::BlokkertVenter { last_error: Some(ref detail) }
            if detail.starts_with("blocked_reason=saksnummer_mangler")
    ));
    assert!(
        arkiv_gateway
            .opprett_journalpost_calls
            .lock()
            .unwrap()
            .is_empty()
    );
    assert!(entity_repo.oppdaterte_dokumenter.lock().unwrap().is_empty());
    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert!(
        events[0]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with("blocked_reason=saksnummer_mangler"))
    );
}

#[tokio::test]
async fn html_template_rendres_for_opprett_journalpost() {
    let command_id = Uuid::new_v4();
    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let dokument_id = Uuid::new_v4();
    let mal_referanse = Uuid::new_v4();
    let envelope = make_internt_notat_command(
        command_id,
        journalpost_client_reference,
        sak_client_reference,
    );

    let entity_repo = FakeEntityTilstandRepository::default();
    entity_repo.sak_med_barn.lock().unwrap().insert(
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
                    tilstand: DokumentTilstand::AvventerRendring,
                    kilde: DokumentKildeTilstand::HtmlTemplate {
                        mal_referanse,
                        felter: vec![TemplateFelt::Saksnummer],
                        rendered_dokument_referanse: None,
                    },
                }],
            }],
        },
    );

    let arkiv_gateway = FakeArkivGateway::default();
    let id_mapping = FakeIdMappingRepository::default();
    id_mapping
        .sak_mapping
        .lock()
        .unwrap()
        .insert(sak_client_reference, sak_id);
    id_mapping
        .journalpost_mapping
        .lock()
        .unwrap()
        .insert(journalpost_client_reference, journalpost_id);

    let service = EksekverKommandoService::new(
        Box::new(entity_repo.clone()),
        Box::new(arkiv_gateway.clone()),
        Box::new(FakeDokumentRenderer),
        Box::new(FakeDokumentLager::with_file(DokumentFil {
            id: mal_referanse,
            data: b"<html>{{saksnummer}}</html>".to_vec(),
            filename: Some("template.html".to_string()),
            content_type: Some("text/html".to_string()),
            metadata: Default::default(),
        })),
        Box::new(FakeStatusPublisher::default()),
        Box::new(FakeDonePublisher),
        Box::new(id_mapping),
        Box::new(FakeStatusProjector),
        Box::new(FakeWakeup::default()),
    );

    let outcome = service.handle(envelope, 1).await.unwrap();

    assert_eq!(outcome, ExecutionOutcome::Klar);
    assert!(
        arkiv_gateway
            .opprett_journalpost_calls
            .lock()
            .unwrap()
            .is_empty(),
        "journalpost must not be created before HTML-template hoveddokument is rendered"
    );
    let sak = entity_repo
        .sak_med_barn
        .lock()
        .unwrap()
        .get(&sak_id)
        .cloned()
        .unwrap();
    let dokument = &sak.journalposter[0].dokumenter[0];
    assert_eq!(dokument.tilstand, DokumentTilstand::Ok);
    assert!(matches!(
        dokument.kilde,
        DokumentKildeTilstand::HtmlTemplate {
            rendered_dokument_referanse: Some(_),
            ..
        }
    ));
}

#[tokio::test]
async fn html_template_med_rendered_referanse_fullfoerer_retry_uten_rendering() {
    let command_id = Uuid::new_v4();
    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let dokument_id = Uuid::new_v4();
    let mal_referanse = Uuid::new_v4();
    let rendered_dokument_referanse = Uuid::new_v4();
    let envelope = make_internt_notat_command(
        command_id,
        journalpost_client_reference,
        sak_client_reference,
    );

    let entity_repo = FakeEntityTilstandRepository::default();
    entity_repo.sak_med_barn.lock().unwrap().insert(
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
                    tilstand: DokumentTilstand::AvventerRendring,
                    kilde: DokumentKildeTilstand::HtmlTemplate {
                        mal_referanse,
                        felter: vec![TemplateFelt::Saksnummer],
                        rendered_dokument_referanse: Some(rendered_dokument_referanse),
                    },
                }],
            }],
        },
    );

    let arkiv_gateway = FakeArkivGateway::default();
    let id_mapping = FakeIdMappingRepository::default();
    id_mapping
        .sak_mapping
        .lock()
        .unwrap()
        .insert(sak_client_reference, sak_id);
    id_mapping
        .journalpost_mapping
        .lock()
        .unwrap()
        .insert(journalpost_client_reference, journalpost_id);

    let dokument_lager = FakeDokumentLager::default();
    let service = EksekverKommandoService::new(
        Box::new(entity_repo.clone()),
        Box::new(arkiv_gateway.clone()),
        Box::new(PanickingDokumentRenderer),
        Box::new(dokument_lager.clone()),
        Box::new(FakeStatusPublisher::default()),
        Box::new(FakeDonePublisher),
        Box::new(id_mapping),
        Box::new(FakeStatusProjector),
        Box::new(FakeWakeup::default()),
    );

    let outcome = service.handle(envelope.clone(), 1).await.unwrap();

    assert_eq!(outcome, ExecutionOutcome::Klar);
    assert!(
        arkiv_gateway
            .opprett_journalpost_calls
            .lock()
            .unwrap()
            .is_empty(),
        "retry-attempten skal bare fullføre RenderDokument og ikke opprette journalpost"
    );
    assert!(
        dokument_lager.files.lock().unwrap().is_empty(),
        "eksisterende rendered_dokument_referanse skal ikke lagres på nytt"
    );
    assert_eq!(
        entity_repo.oppdaterte_dokumenter.lock().unwrap().as_slice(),
        &[(dokument_id, DokumentTilstand::Ok)]
    );

    let outcome = service.handle(envelope, 2).await.unwrap();

    assert_eq!(outcome, ExecutionOutcome::Klar);
    assert_eq!(
        arkiv_gateway
            .opprett_journalpost_calls
            .lock()
            .unwrap()
            .len(),
        1,
        "neste attempt skal kunne fortsette til OpprettJournalpost"
    );
}

#[tokio::test]
async fn sikri_feil_publiserer_kun_stabil_safe_detail() {
    let command_id = Uuid::new_v4();
    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let envelope = WireCommandEnvelope {
        command_id,
        correlation_id: Some(Uuid::new_v4()),
        payload: WireCommand::OpprettSak(lib_schemas::skuffen::command::sak::OpprettSak {
            client_reference: sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel("Test sak".to_string()),
            ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())
                .unwrap(),
            arkivdel: lib_schemas::skuffen::command::sak::Arkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgjengelighet: Tilgjengelighet::Offentlig,
        }),
    };

    let entity_repo = FakeEntityTilstandRepository::default();
    entity_repo.sak_med_barn.lock().unwrap().insert(
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

    let status_publisher = FakeStatusPublisher::default();
    let id_mapping = FakeIdMappingRepository::default();
    id_mapping
        .sak_mapping
        .lock()
        .unwrap()
        .insert(sak_client_reference, sak_id);
    let service = EksekverKommandoService::new(
        Box::new(entity_repo),
        Box::new(FakeArkivGateway {
            opprett_sak_error: Some(
                "sikri_recoverability=irrecoverable sikri_unknown_user (method=POST, url=https://example.invalid/api, user=Z12345)".to_string(),
            ),
            ..FakeArkivGateway::default()
        }),
        Box::new(FakeDokumentRenderer),
        Box::new(FakeDokumentLager::default()),
        Box::new(status_publisher.clone()),
        Box::new(FakeDonePublisher),
        Box::new(id_mapping),
        Box::new(FakeStatusProjector),
        Box::new(FakeWakeup::default()),
    );

    let outcome = service.handle(envelope, 1).await.unwrap();

    assert_eq!(
        outcome,
        ExecutionOutcome::Feil {
            last_error: Some("sikri_unknown_user".to_string())
        }
    );
    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail.as_deref(), Some("sikri_unknown_user"));
}

#[tokio::test]
async fn ukjent_sikri_feil_publiserer_generisk_upstream_safe_detail() {
    let command_id = Uuid::new_v4();
    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let envelope = WireCommandEnvelope {
        command_id,
        correlation_id: Some(Uuid::new_v4()),
        payload: WireCommand::OpprettSak(lib_schemas::skuffen::command::sak::OpprettSak {
            client_reference: sak_client_reference,
            sakstittel: lib_schemas::skuffen::sak::Sakstittel("Test sak".to_string()),
            ordningsverdi: lib_schemas::skuffen::sak::Ordningsverdi::new("123".to_string())
                .unwrap(),
            arkivdel: lib_schemas::skuffen::command::sak::Arkivdel::Tilsynsdivisjonene,
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgjengelighet: Tilgjengelighet::Offentlig,
        }),
    };

    let entity_repo = FakeEntityTilstandRepository::default();
    entity_repo.sak_med_barn.lock().unwrap().insert(
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

    let status_publisher = FakeStatusPublisher::default();
    let id_mapping = FakeIdMappingRepository::default();
    id_mapping
        .sak_mapping
        .lock()
        .unwrap()
        .insert(sak_client_reference, sak_id);
    let service = EksekverKommandoService::new(
        Box::new(entity_repo),
        Box::new(FakeArkivGateway {
            opprett_sak_error: Some(
                "connection failed for https://example.invalid/api?user=Z12345".to_string(),
            ),
            ..FakeArkivGateway::default()
        }),
        Box::new(FakeDokumentRenderer),
        Box::new(FakeDokumentLager::default()),
        Box::new(status_publisher.clone()),
        Box::new(FakeDonePublisher),
        Box::new(id_mapping),
        Box::new(FakeStatusProjector),
        Box::new(FakeWakeup::default()),
    );

    let outcome = service.handle(envelope, 1).await.unwrap();

    assert_eq!(
        outcome,
        ExecutionOutcome::Retrying {
            last_error: Some("execution_error".to_string())
        }
    );
    let events = status_publisher.events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].detail.as_deref(), Some("execution_error"));
}

#[tokio::test]
async fn worker_materialiserer_ready_etter_operasjon_som_klar() {
    let command_id = Uuid::new_v4();
    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let dokument_id = Uuid::new_v4();
    let envelope = make_internt_notat_command(
        command_id,
        journalpost_client_reference,
        sak_client_reference,
    );

    let entity_repo = FakeEntityTilstandRepository::default();
    entity_repo.sak_med_barn.lock().unwrap().insert(
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

    let arkiv_gateway = FakeArkivGateway::default();
    let id_mapping = FakeIdMappingRepository::default();
    id_mapping
        .sak_mapping
        .lock()
        .unwrap()
        .insert(sak_client_reference, sak_id);
    id_mapping
        .journalpost_mapping
        .lock()
        .unwrap()
        .insert(journalpost_client_reference, journalpost_id);
    let wakeup = FakeWakeup::default();
    let executor = build_executor(entity_repo, arkiv_gateway, id_mapping, wakeup);

    let execution_repo = FakeExecutionRepository::default();
    execution_repo.data.lock().unwrap().next_command = Some(EksekveringKommando {
        command_id,
        envelope,
        attempt_no: 0,
        utfores_venter_publisert: true,
    });

    let worker = EksekveringWorker::new(
        Box::new(execution_repo.clone()),
        executor,
        "worker-1".to_string(),
        tokio::time::Duration::from_millis(1),
    );

    let envelope = execution_repo
        .data
        .lock()
        .unwrap()
        .next_command
        .as_ref()
        .unwrap()
        .envelope
        .clone();
    worker.execute_one(command_id, envelope).await.unwrap();

    let data = execution_repo.data.lock().unwrap();
    assert_eq!(data.markert_kjorer, vec![command_id]);
    assert_eq!(data.markert_klar, vec![command_id]);
    assert!(data.markert_ok.is_empty());
    assert!(data.markert_blokkert.is_empty());
    assert!(data.markert_feil.is_empty());
}

fn build_executor(
    entity_repo: FakeEntityTilstandRepository,
    arkiv_gateway: FakeArkivGateway,
    id_mapping: FakeIdMappingRepository,
    wakeup: FakeWakeup,
) -> EksekverKommandoService {
    EksekverKommandoService::new(
        Box::new(entity_repo),
        Box::new(arkiv_gateway),
        Box::new(FakeDokumentRenderer),
        Box::new(FakeDokumentLager::default()),
        Box::new(FakeStatusPublisher::default()),
        Box::new(FakeDonePublisher),
        Box::new(id_mapping),
        Box::new(FakeStatusProjector),
        Box::new(wakeup),
    )
}

fn make_internt_notat_command(
    command_id: Uuid,
    journalpost_client_reference: Uuid,
    sak_client_reference: Uuid,
) -> ApplicationCommandEnvelope<ApplicationCommand> {
    crate::command::test_support::map_wire_envelope(WireCommandEnvelope {
        command_id,
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
                    client_reference: Uuid::new_v4(),
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

#[tokio::test]
async fn statisk_html_template_rendres_uten_saksnummer_og_utsetter_journalpost() {
    let command_id = Uuid::new_v4();
    let sak_client_reference = Uuid::new_v4();
    let sak_id = Uuid::new_v4();
    let journalpost_client_reference = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let dokument_id = Uuid::new_v4();
    let mal_referanse = Uuid::new_v4();
    let envelope = make_internt_notat_command(
        command_id,
        journalpost_client_reference,
        sak_client_reference,
    );

    let entity_repo = FakeEntityTilstandRepository::default();
    entity_repo.sak_med_barn.lock().unwrap().insert(
        sak_id,
        SakMedBarn {
            sak_id: SkuffenSakId::from(sak_id),
            tilstand: SakTilstand::Opprettet,
            sikri_id: Some(100),
            saksnummer: None,
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
                    tilstand: DokumentTilstand::AvventerRendring,
                    kilde: DokumentKildeTilstand::HtmlTemplate {
                        mal_referanse,
                        felter: vec![],
                        rendered_dokument_referanse: None,
                    },
                }],
            }],
        },
    );

    let arkiv_gateway = FakeArkivGateway::default();
    let id_mapping = FakeIdMappingRepository::default();
    id_mapping
        .sak_mapping
        .lock()
        .unwrap()
        .insert(sak_client_reference, sak_id);
    id_mapping
        .journalpost_mapping
        .lock()
        .unwrap()
        .insert(journalpost_client_reference, journalpost_id);

    let dokument_lager = FakeDokumentLager::with_file(DokumentFil {
        id: mal_referanse,
        data: b"<html><body>Statisk HTML</body></html>".to_vec(),
        filename: Some("static_template.html".to_string()),
        content_type: Some("text/html".to_string()),
        metadata: Default::default(),
    });

    let service = EksekverKommandoService::new(
        Box::new(entity_repo.clone()),
        Box::new(arkiv_gateway.clone()),
        Box::new(FakeDokumentRenderer),
        Box::new(dokument_lager.clone()),
        Box::new(FakeStatusPublisher::default()),
        Box::new(FakeDonePublisher),
        Box::new(id_mapping),
        Box::new(FakeStatusProjector),
        Box::new(FakeWakeup::default()),
    );

    let outcome = service.handle(envelope, 1).await.unwrap();

    assert_eq!(outcome, ExecutionOutcome::Klar);
    assert!(
        arkiv_gateway
            .opprett_journalpost_calls
            .lock()
            .unwrap()
            .is_empty(),
        "journalpost must not be created in render-only attempt"
    );
    let sak = entity_repo
        .sak_med_barn
        .lock()
        .unwrap()
        .get(&sak_id)
        .cloned()
        .unwrap();
    let dokument = &sak.journalposter[0].dokumenter[0];
    assert_eq!(dokument.tilstand, DokumentTilstand::Ok);
    let rendered_id = match &dokument.kilde {
        DokumentKildeTilstand::HtmlTemplate {
            rendered_dokument_referanse: Some(rendered_id),
            ..
        } => *rendered_id,
        _ => panic!("rendered dokument reference must be stored"),
    };
    let files = dokument_lager.files.lock().unwrap();
    let rendered_file = files
        .get(&rendered_id)
        .expect("rendered document must be saved in lager");
    assert_eq!(
        rendered_file.content_type.as_deref(),
        Some("application/pdf")
    );
    assert_eq!(
        rendered_file.filename.as_deref(),
        Some(&*format!("{rendered_id}.pdf"))
    );
    assert_eq!(
        rendered_file.metadata.origin.as_deref(),
        Some("skuffen_html_template_renderer")
    );
    assert_eq!(
        rendered_file.metadata.source_template_reference,
        Some(mal_referanse)
    );
    assert_eq!(rendered_file.metadata.source_document_id, Some(dokument_id));
    assert_eq!(rendered_file.metadata.source_command_id, Some(command_id));
}
