//! Tester for worker-løkka (SKU-0020).
//!
//! `OperasjonWorker` og det slettede `EvaluerOperasjonerService` hadde ingen
//! tester. Det var hullet som lot terminale utfall forsvinne uten event for
//! hele kommandoklasser.
//!
//! Testene går gjennom `run()`, ikke gjennom private metoder: det som skal
//! være sant, er sant om den som starter workeren — ikke om et hjelpekall.
//! Nedstengingssignalet gis av repositoryet når køen spørres, slik at hver
//! test er deterministisk uten å vente på klokka.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::operasjon::{
    EntitetId, Operasjon, OperasjonId, OperasjonSammendrag, Operasjonsstatus, Operasjonstype,
};
use domain::eksekvering::tilstand::{SakMedBarn, SakTilstand, Saksansvarlig};
use domain::eksekvering::typer::{
    CommandEvent, CommandStatus, CommandTypeCode, EksekveringFeil, Operasjonshendelse,
    Operasjonstatus, Statuskontekst,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::command::materialisering::{
    Dekomponeringsplan, DokumentAttributter, JournalpostAttributter, SakAttributter,
};
use crate::command::ports::eksekvering_port::{
    ArkivGateway, Journalstatus, ObservertJournalstatus, OpprettJournalpostResultat,
    OpprettSakResultat,
};
use crate::command::ports::fakta_port::FaktaRepository;
use crate::command::ports::operasjon_port::{
    CommandMetadata, CommandOutcome, Dekomponeringsresultat, ExecutorLease, Faktaoppdatering,
    Gjenoppretting, OperasjonRepository,
};
use crate::command::ports::status_publisher_port::StatusPublisher;
use crate::command::services::eksekver_operasjon::{EksekverOperasjonService, RenderOperasjon};
use crate::command::services::operasjon_worker::{OperasjonWorker, WorkerInnstillinger};

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

/// En rad i en varslingskø, med markeringen som avgjør om den hentes igjen.
#[derive(Clone)]
struct Varselrad {
    operasjon: Operasjon,
    varslet: bool,
}

#[derive(Default)]
struct Tilstand {
    lederskap: bool,
    kjorbare: VecDeque<Operasjon>,
    krever_avklaring: Vec<Varselrad>,
    varselkandidater: Vec<Varselrad>,
    blokkerte: Vec<OperasjonId>,
    plukk: usize,
    laasforsok: usize,
    markering_feiler: bool,
    /// Lar en test si at en søskenoperasjon allerede har feilet terminalt.
    command_outcome_overstyring: Option<CommandOutcome>,
    /// Signalet workeren stenger ned på. Byttes ut mellom to «oppstarter».
    stopp: Option<CancellationToken>,
}

/// Repository som stanser workeren i det køen spørres, slik at én `run()`
/// tilsvarer én gjennomkjøring av løkka.
#[derive(Clone, Default)]
struct FakeOperasjonRepo {
    tilstand: Arc<Mutex<Tilstand>>,
}

struct IngenLease;
impl ExecutorLease for IngenLease {}

impl FakeOperasjonRepo {
    fn med_lederskap() -> Self {
        let repo = Self::default();
        repo.tilstand.lock().unwrap().lederskap = true;
        repo
    }

    fn sett_stoppsignal(&self, stopp: CancellationToken) {
        self.tilstand.lock().unwrap().stopp = Some(stopp);
    }

    fn koe(&self, operasjoner: Vec<Operasjon>) {
        self.tilstand.lock().unwrap().kjorbare = operasjoner.into();
    }

    fn krever_avklaring(&self, operasjoner: Vec<(Operasjon, bool)>) {
        self.tilstand.lock().unwrap().krever_avklaring = operasjoner
            .into_iter()
            .map(|(operasjon, varslet)| Varselrad { operasjon, varslet })
            .collect();
    }

    fn varselkandidater(&self, operasjoner: Vec<Operasjon>) {
        self.tilstand.lock().unwrap().varselkandidater = operasjoner
            .into_iter()
            .map(|operasjon| Varselrad {
                operasjon,
                varslet: false,
            })
            .collect();
    }

    fn la_markering_feile(&self, feiler: bool) {
        self.tilstand.lock().unwrap().markering_feiler = feiler;
    }

    fn sett_command_outcome(&self, utfall: CommandOutcome) {
        self.tilstand.lock().unwrap().command_outcome_overstyring = Some(utfall);
    }

    fn antall_plukk(&self) -> usize {
        self.tilstand.lock().unwrap().plukk
    }

    fn antall_laasforsok(&self) -> usize {
        self.tilstand.lock().unwrap().laasforsok
    }

    fn gjenstaaende_i_koe(&self) -> usize {
        self.tilstand.lock().unwrap().kjorbare.len()
    }

    fn uvarslede_avklaringer(&self) -> usize {
        self.tilstand
            .lock()
            .unwrap()
            .krever_avklaring
            .iter()
            .filter(|rad| !rad.varslet)
            .count()
    }

    fn blokkerte(&self) -> Vec<OperasjonId> {
        self.tilstand.lock().unwrap().blokkerte.clone()
    }
}

#[async_trait]
impl OperasjonRepository for FakeOperasjonRepo {
    async fn try_acquire_executor_lock(
        &self,
        _executor_id: &str,
    ) -> Result<Option<Box<dyn ExecutorLease>>, anyhow::Error> {
        let mut tilstand = self.tilstand.lock().unwrap();
        tilstand.laasforsok += 1;
        if tilstand.lederskap {
            Ok(Some(Box::new(IngenLease)))
        } else {
            // Uten lederskap venter workeren. Stopper vi den her, avsluttes
            // den uten å ha rørt køen.
            if let Some(stopp) = tilstand.stopp.as_ref() {
                stopp.cancel();
            }
            Ok(None)
        }
    }

    async fn lagre_dekomponering(
        &self,
        _plan: Dekomponeringsplan,
    ) -> Result<Dekomponeringsresultat, anyhow::Error> {
        Ok(Dekomponeringsresultat { nye_operasjoner: 0 })
    }

    async fn hent_neste_kjorbare(&self) -> Result<Option<Operasjon>, anyhow::Error> {
        let mut tilstand = self.tilstand.lock().unwrap();
        tilstand.plukk += 1;
        if let Some(stopp) = tilstand.stopp.as_ref() {
            stopp.cancel();
        }
        Ok(tilstand.kjorbare.pop_front())
    }

    async fn marker_kjorer(
        &self,
        _operasjon_id: OperasjonId,
        _executor_id: &str,
    ) -> Result<i32, anyhow::Error> {
        Ok(1)
    }

    async fn marker_sendt(
        &self,
        _operasjon_id: OperasjonId,
        _attempt_no: i32,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn fullfor_ok(
        &self,
        _operasjon_id: OperasjonId,
        _attempt_no: i32,
        _oppdatering: Faktaoppdatering,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn fullfor_poll(
        &self,
        _operasjon_id: OperasjonId,
        _attempt_no: i32,
        _oppdatering: Faktaoppdatering,
        _neste_forsok_at: DateTime<Utc>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn marker_retry(
        &self,
        _operasjon_id: OperasjonId,
        _attempt_no: i32,
        _detalj: &str,
        _neste_forsok_at: DateTime<Utc>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn marker_feilet(
        &self,
        _operasjon_id: OperasjonId,
        _attempt_no: i32,
        _detalj: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn marker_blokkert(
        &self,
        operasjon_id: OperasjonId,
        _attempt_no: Option<i32>,
        _detalj: &str,
    ) -> Result<(), anyhow::Error> {
        self.tilstand.lock().unwrap().blokkerte.push(operasjon_id);
        Ok(())
    }

    async fn gjenopprett_etter_restart(&self) -> Result<Gjenoppretting, anyhow::Error> {
        Ok(Gjenoppretting::default())
    }

    async fn hent_krever_avklaring(&self) -> Result<Vec<Operasjon>, anyhow::Error> {
        Ok(self
            .tilstand
            .lock()
            .unwrap()
            .krever_avklaring
            .iter()
            .filter(|rad| !rad.varslet)
            .map(|rad| rad.operasjon)
            .collect())
    }

    async fn marker_avklaring_varslet(
        &self,
        operasjon_id: OperasjonId,
    ) -> Result<(), anyhow::Error> {
        let mut tilstand = self.tilstand.lock().unwrap();
        if tilstand.markering_feiler {
            anyhow::bail!("markering feilet");
        }
        for rad in tilstand.krever_avklaring.iter_mut() {
            if rad.operasjon.operasjon_id == operasjon_id {
                rad.varslet = true;
            }
        }
        Ok(())
    }

    async fn hent_sammendrag_for_sak(
        &self,
        _sak_id: SkuffenSakId,
    ) -> Result<Vec<OperasjonSammendrag>, anyhow::Error> {
        Ok(Vec::new())
    }

    async fn hent_command_metadata(
        &self,
        _operasjon_id: OperasjonId,
    ) -> Result<CommandMetadata, anyhow::Error> {
        Ok(CommandMetadata {
            command_id: COMMAND_ID,
            correlation_id: None,
            command_type: CommandTypeCode::SettSaksansvarlig,
            kontekst: Statuskontekst::default(),
        })
    }

    async fn hent_status(
        &self,
        _operasjon_id: OperasjonId,
    ) -> Result<Option<Operasjonsstatus>, anyhow::Error> {
        Ok(None)
    }

    /// Folden slik databasen ville regnet den: en umarkert
    /// `krever_avklaring`-rad gir `KreverAvklaring`.
    async fn hent_command_outcome(
        &self,
        _command_id: Uuid,
    ) -> Result<CommandOutcome, anyhow::Error> {
        Ok(*self
            .tilstand
            .lock()
            .unwrap()
            .command_outcome_overstyring
            .as_ref()
            .unwrap_or(&CommandOutcome::KreverAvklaring))
    }

    async fn hent_varselkandidater(
        &self,
        _eldre_enn: DateTime<Utc>,
    ) -> Result<Vec<Operasjon>, anyhow::Error> {
        Ok(self
            .tilstand
            .lock()
            .unwrap()
            .varselkandidater
            .iter()
            .filter(|rad| !rad.varslet)
            .map(|rad| rad.operasjon)
            .collect())
    }

    async fn marker_varslet(&self, operasjon_id: OperasjonId) -> Result<(), anyhow::Error> {
        let mut tilstand = self.tilstand.lock().unwrap();
        for rad in tilstand.varselkandidater.iter_mut() {
            if rad.operasjon.operasjon_id == operasjon_id {
                rad.varslet = true;
            }
        }
        Ok(())
    }
}

/// Fakta uten saksnummer. `SettSaksansvarlig` blir da `Blokkert`, så
/// executoren aldri når arkivet.
#[derive(Clone)]
struct BlokkerendeFaktaRepository;

#[async_trait]
impl FaktaRepository for BlokkerendeFaktaRepository {
    async fn hent_sak_med_barn(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<SakMedBarn>, anyhow::Error> {
        Ok(Some(SakMedBarn {
            sak_id,
            tilstand: SakTilstand::IkkeOpprettet,
            arkiv_id: None,
            oensket_saksansvarlig: Some(Saksansvarlig {
                saksbehandler_id: "Z12345".to_string(),
                enhet: "MT-1".to_string(),
            }),
            naavaerende_saksansvarlig: None,
            journalposter: Vec::new(),
        }))
    }

    async fn hent_sak_attributter(
        &self,
        _sak_id: SkuffenSakId,
    ) -> Result<Option<SakAttributter>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_journalpost_attributter(
        &self,
        _journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<JournalpostAttributter>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_dokument_attributter(
        &self,
        _dokument_id: SkuffenDokumentId,
    ) -> Result<Option<DokumentAttributter>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_dokumenter_for_journalpost(
        &self,
        _journalpost_id: SkuffenJournalpostId,
    ) -> Result<Vec<(SkuffenDokumentId, DokumentAttributter)>, anyhow::Error> {
        Ok(Vec::new())
    }
}

/// Arkivet skal ikke røres av worker-testene. Gjør det det likevel, er det en
/// feil i beslutningsstien, ikke i testen.
struct UroertArkivGateway;

#[async_trait]
impl ArkivGateway for UroertArkivGateway {
    async fn opprett_sak(
        &self,
        _attributter: &SakAttributter,
    ) -> Result<OpprettSakResultat, EksekveringFeil> {
        unreachable!("worker-testene skal aldri nå arkivet")
    }

    async fn opprett_journalpost(
        &self,
        _saksnummer: &str,
        _journalpost: &JournalpostAttributter,
        _hoveddokument: &DokumentAttributter,
    ) -> Result<OpprettJournalpostResultat, EksekveringFeil> {
        unreachable!("worker-testene skal aldri nå arkivet")
    }

    async fn legg_til_vedlegg(
        &self,
        _journalpost_id: i32,
        _vedlegg: &DokumentAttributter,
    ) -> Result<Option<i32>, EksekveringFeil> {
        unreachable!("worker-testene skal aldri nå arkivet")
    }

    async fn sett_journalpost_status(
        &self,
        _journalpost_id: i32,
        _status: Journalstatus,
    ) -> Result<(), EksekveringFeil> {
        unreachable!("worker-testene skal aldri nå arkivet")
    }

    async fn avskriv_journalpost(
        &self,
        _journalpost_id: i32,
        _kildesystem: Option<&str>,
        _merknad: Option<&str>,
    ) -> Result<(), EksekveringFeil> {
        unreachable!("worker-testene skal aldri nå arkivet")
    }

    async fn hent_journalstatus(
        &self,
        _journalpost_id: i32,
    ) -> Result<ObservertJournalstatus, EksekveringFeil> {
        unreachable!("worker-testene skal aldri nå arkivet")
    }

    async fn avslutt_sak(&self, _saksnummer: &str) -> Result<(), EksekveringFeil> {
        unreachable!("worker-testene skal aldri nå arkivet")
    }

    async fn sett_saksansvarlig(
        &self,
        _saksnummer: &str,
        _saksbehandler_id: &str,
        _saksbehandler_enhet: &str,
    ) -> Result<(), EksekveringFeil> {
        unreachable!("worker-testene skal aldri nå arkivet")
    }
}

struct UbruktRenderOperasjon;

#[async_trait]
impl RenderOperasjon for UbruktRenderOperasjon {
    async fn render(
        &self,
        _dokument_id: SkuffenDokumentId,
        _mal_referanse: Uuid,
        _felter: &[domain::eksekvering::html_template::TemplateFelt],
        _saksnummer: Option<&str>,
    ) -> Result<Uuid, EksekveringFeil> {
        unreachable!("render brukes ikke i worker-testene")
    }
}

#[derive(Clone, Default)]
struct FakeStatusPublisher {
    operasjonstatuser: Arc<Mutex<Vec<Operasjonstatus>>>,
    commandstatuser: Arc<Mutex<Vec<CommandStatus>>>,
}

impl FakeStatusPublisher {
    fn command_hendelser(&self, hendelse: CommandEvent) -> Vec<CommandStatus> {
        self.commandstatuser
            .lock()
            .unwrap()
            .iter()
            .filter(|status| status.hendelse == hendelse)
            .cloned()
            .collect()
    }

    fn hendelser(&self, hendelse: Operasjonshendelse) -> Vec<Operasjonstatus> {
        self.operasjonstatuser
            .lock()
            .unwrap()
            .iter()
            .filter(|status| status.hendelse == hendelse)
            .cloned()
            .collect()
    }
}

#[async_trait]
impl StatusPublisher for FakeStatusPublisher {
    async fn publiser_command_status(&self, status: CommandStatus) -> Result<(), anyhow::Error> {
        self.commandstatuser.lock().unwrap().push(status);
        Ok(())
    }

    async fn publiser_operasjonstatus(&self, status: Operasjonstatus) -> Result<(), anyhow::Error> {
        self.operasjonstatuser.lock().unwrap().push(status);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Oppsett
// ---------------------------------------------------------------------------

const COMMAND_ID: Uuid = Uuid::from_u128(42);

fn sak_id() -> SkuffenSakId {
    SkuffenSakId::from(Uuid::from_u128(1))
}

fn operasjon(nummer: u128) -> Operasjon {
    Operasjon {
        operasjon_id: OperasjonId(Uuid::from_u128(nummer)),
        operasjonstype: Operasjonstype::SettSaksansvarlig,
        entitet_id: EntitetId::Sak(sak_id()),
        sak_id: sak_id(),
    }
}

/// Intervallene er null eller nesten null: nedstengingen kommer fra
/// repositoryet, ikke fra klokka.
fn innstillinger() -> WorkerInnstillinger {
    WorkerInnstillinger {
        varselintervall: Duration::ZERO,
        tomgangspause: Duration::from_millis(1),
        lederforsok_intervall: Duration::from_millis(1),
        varselfrist: chrono::Duration::hours(24),
    }
}

/// Én «oppstart» av workeren. Repositoryet får et ferskt stoppsignal, slik at
/// en påfølgende oppstart ikke arver forrige nedstenging.
async fn kjor(
    repo: &FakeOperasjonRepo,
    publisher: &FakeStatusPublisher,
) -> Result<(), anyhow::Error> {
    let shutdown = CancellationToken::new();
    repo.sett_stoppsignal(shutdown.clone());

    let executor = EksekverOperasjonService::new(
        Box::new(repo.clone()),
        Box::new(BlokkerendeFaktaRepository),
        Box::new(UroertArkivGateway),
        Box::new(UbruktRenderOperasjon),
        Box::new(publisher.clone()),
        "test-executor",
        Duration::from_millis(1),
    );

    OperasjonWorker::new(
        executor,
        Arc::new(repo.clone()),
        Arc::new(publisher.clone()),
        "test-executor",
        innstillinger(),
        shutdown,
    )
    .run()
    .await
}

// ---------------------------------------------------------------------------
// Lederskap og nedstenging
// ---------------------------------------------------------------------------

#[tokio::test]
async fn uten_lederskap_plukkes_ingen_operasjon() {
    let repo = FakeOperasjonRepo::default();
    let publisher = FakeStatusPublisher::default();
    repo.koe(vec![operasjon(2)]);

    kjor(&repo, &publisher).await.expect("avsluttet rent");

    assert!(repo.antall_laasforsok() >= 1, "lederskap må forsøkes");
    assert_eq!(
        repo.antall_plukk(),
        0,
        "en instans uten lederskap skal ikke røre køen"
    );
    assert_eq!(repo.gjenstaaende_i_koe(), 1);
}

#[tokio::test]
async fn nedstenging_midt_i_loekka_plukker_ikke_ny_operasjon() {
    let repo = FakeOperasjonRepo::med_lederskap();
    let publisher = FakeStatusPublisher::default();
    repo.koe(vec![operasjon(2), operasjon(3), operasjon(4)]);

    kjor(&repo, &publisher).await.expect("avsluttet rent");

    assert_eq!(
        repo.antall_plukk(),
        1,
        "workeren skal ikke plukke en ny operasjon etter nedstengingssignalet"
    );
    assert_eq!(repo.blokkerte(), vec![OperasjonId(Uuid::from_u128(2))]);
    assert_eq!(repo.gjenstaaende_i_koe(), 2);
}

// ---------------------------------------------------------------------------
// Varsling om ukjent utfall (SKU-0020 R6)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn kun_umarkerte_rader_varsles_som_krever_avklaring() {
    let repo = FakeOperasjonRepo::med_lederskap();
    let publisher = FakeStatusPublisher::default();
    repo.krever_avklaring(vec![(operasjon(10), true), (operasjon(11), false)]);

    kjor(&repo, &publisher).await.expect("avsluttet rent");

    let varsler = publisher.hendelser(Operasjonshendelse::KreverAvklaring);
    assert_eq!(
        varsler.len(),
        1,
        "en allerede varslet rad skal ikke gjentas"
    );
    assert_eq!(varsler[0].operasjon_id, OperasjonId(Uuid::from_u128(11)));
}

#[tokio::test]
async fn andre_oppstart_republiserer_ikke_krever_avklaring() {
    let repo = FakeOperasjonRepo::med_lederskap();
    let publisher = FakeStatusPublisher::default();
    repo.krever_avklaring(vec![(operasjon(11), false)]);

    kjor(&repo, &publisher).await.expect("første oppstart");
    kjor(&repo, &publisher).await.expect("andre oppstart");

    assert_eq!(
        publisher
            .hendelser(Operasjonshendelse::KreverAvklaring)
            .len(),
        1,
        "markeringen i databasen, ikke antall flyttede rader, styrer varslingen"
    );
    assert_eq!(repo.uvarslede_avklaringer(), 0);
}

#[tokio::test]
async fn krasj_mellom_publisering_og_markering_gir_duplikat_ikke_tap() {
    let repo = FakeOperasjonRepo::med_lederskap();
    let publisher = FakeStatusPublisher::default();
    repo.krever_avklaring(vec![(operasjon(11), false)]);

    repo.la_markering_feile(true);
    kjor(&repo, &publisher)
        .await
        .expect_err("markeringen feilet, så oppstarten skal feile");
    assert_eq!(
        publisher
            .hendelser(Operasjonshendelse::KreverAvklaring)
            .len(),
        1
    );
    assert_eq!(
        repo.uvarslede_avklaringer(),
        1,
        "raden er ikke markert, så den skal varsles på nytt"
    );

    repo.la_markering_feile(false);
    kjor(&repo, &publisher).await.expect("andre oppstart");

    assert_eq!(
        publisher
            .hendelser(Operasjonshendelse::KreverAvklaring)
            .len(),
        2,
        "at-least-once: ett duplikat er riktig utfall, tap er det ikke"
    );
    assert_eq!(repo.uvarslede_avklaringer(), 0);
}

// ---------------------------------------------------------------------------
// Advisory 24-timersvarsel (D11)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn varselkandidat_publiseres_kun_en_gang_noensinne() {
    let repo = FakeOperasjonRepo::med_lederskap();
    let publisher = FakeStatusPublisher::default();
    repo.varselkandidater(vec![operasjon(20)]);

    kjor(&repo, &publisher).await.expect("første oppstart");
    kjor(&repo, &publisher).await.expect("andre oppstart");

    let varsler = publisher.hendelser(Operasjonshendelse::Varsel);
    assert_eq!(
        varsler.len(),
        1,
        "`varslet_at` settes og nullstilles aldri, så varselet kommer én gang"
    );
    assert_eq!(varsler[0].operasjon_id, OperasjonId(Uuid::from_u128(20)));
}

// ---------------------------------------------------------------------------
// KreverAvklaring på kommandonivå
// ---------------------------------------------------------------------------

#[tokio::test]
async fn ukjent_utfall_naar_klienten_paa_command_subjectet() {
    let repo = FakeOperasjonRepo::med_lederskap();
    let publisher = FakeStatusPublisher::default();
    repo.krever_avklaring(vec![(operasjon(11), false)]);

    kjor(&repo, &publisher).await.expect("avsluttet rent");

    let command = publisher.command_hendelser(CommandEvent::KreverAvklaring);
    assert_eq!(
        command.len(),
        1,
        "en klient som følger `.command` skal ikke se stillhet"
    );
    assert!(
        !command[0].terminal,
        "utfallet er ikke avgjort — operasjonen kan bli ok etter admin write"
    );
    assert_eq!(command[0].command_id, COMMAND_ID);
}

#[tokio::test]
async fn allerede_feilet_kommando_faar_ikke_krever_avklaring_i_tillegg() {
    let repo = FakeOperasjonRepo::med_lederskap();
    let publisher = FakeStatusPublisher::default();
    repo.krever_avklaring(vec![(operasjon(11), false)]);
    // En søskenoperasjon feilet terminalt. `Feilet` er monotont og kan ikke
    // trekkes tilbake av et uavklart søsken.
    repo.sett_command_outcome(CommandOutcome::Feilet);

    kjor(&repo, &publisher).await.expect("avsluttet rent");

    assert!(
        publisher
            .command_hendelser(CommandEvent::KreverAvklaring)
            .is_empty(),
        "et terminalt Feilet skal ikke etterfølges av et ikke-terminalt utfall"
    );
    assert_eq!(
        publisher
            .hendelser(Operasjonshendelse::KreverAvklaring)
            .len(),
        1,
        "operasjonsnivået skal fortsatt varsles"
    );
}
