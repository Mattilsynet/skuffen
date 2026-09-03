//! Regresjonstester for feilklassifiseringen i eksekveringen.
//!
//! Kjernen er SKU-0016 R6: recoverable feil retryes for alltid med backoff,
//! og kun irrecoverable feil gir terminal `feilet`.
//!
//! Frem til nå var regelen ikke implementert. Executoren mappet hvert
//! arkivkall gjennom en `recoverable()`-hjelper som kastet klassifiseringen
//! og gjorde alt fra arkivet retrybart. Testene under går gjennom
//! `EksekverOperasjonService::execute` — altså kallveien — nettopp fordi det
//! var isolerte tester av mappingfunksjoner som lot defekten overleve.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::operasjon::{
    EntitetId, Operasjon, OperasjonId, OperasjonSammendrag, Operasjonsstatus, Operasjonstype,
};
use domain::eksekvering::tilstand::{
    JournalpostMedDokumenter, JournalpostTilstand, JournalpostType, SakMedBarn, SakTilstand,
    Saksansvarlig,
};
use domain::eksekvering::typer::{
    CommandTypeCode, EksekveringFeil, Operasjonshendelse, Operasjonstatus, StatusErrorCode,
    Statuskontekst,
};
use uuid::Uuid;

use crate::command::materialisering::{
    Dekomponeringsplan, DokumentAttributter, JournalpostAttributter, Korrespondanseparter,
    SakAttributter, Tilgang,
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

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

/// Det executoren faktisk skrev, i den rekkefølgen det skjedde.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Skriving {
    Kjorer,
    Sendt,
    Ok,
    Poll { neste_forsok_at: DateTime<Utc> },
    Retry { detalj: String },
    Feilet { detalj: String },
    Blokkert { detalj: String },
}

#[derive(Clone)]
struct FakeOperasjonRepository {
    skrivinger: Arc<Mutex<Vec<Skriving>>>,
    outcome: Arc<Mutex<CommandOutcome>>,
}

impl Default for FakeOperasjonRepository {
    fn default() -> Self {
        Self {
            skrivinger: Arc::new(Mutex::new(Vec::new())),
            // Uavklart: kommandostatus folder vi ikke over i disse testene.
            outcome: Arc::new(Mutex::new(CommandOutcome::Uavklart)),
        }
    }
}

impl FakeOperasjonRepository {
    fn skrivinger(&self) -> Vec<Skriving> {
        self.skrivinger.lock().unwrap().clone()
    }

    fn push(&self, skriving: Skriving) {
        self.skrivinger.lock().unwrap().push(skriving);
    }
}

#[async_trait]
impl OperasjonRepository for FakeOperasjonRepository {
    async fn try_acquire_executor_lock(
        &self,
        _executor_id: &str,
    ) -> Result<Option<Box<dyn ExecutorLease>>, anyhow::Error> {
        Ok(None)
    }

    async fn lagre_dekomponering(
        &self,
        _plan: Dekomponeringsplan,
    ) -> Result<Dekomponeringsresultat, anyhow::Error> {
        Ok(Dekomponeringsresultat { nye_operasjoner: 0 })
    }

    async fn hent_neste_kjorbare(&self) -> Result<Option<Operasjon>, anyhow::Error> {
        Ok(None)
    }

    async fn marker_kjorer(
        &self,
        _operasjon_id: OperasjonId,
        _executor_id: &str,
    ) -> Result<i32, anyhow::Error> {
        self.push(Skriving::Kjorer);
        Ok(1)
    }

    async fn marker_sendt(
        &self,
        _operasjon_id: OperasjonId,
        _attempt_no: i32,
    ) -> Result<(), anyhow::Error> {
        self.push(Skriving::Sendt);
        Ok(())
    }

    async fn fullfor_ok(
        &self,
        _operasjon_id: OperasjonId,
        _attempt_no: i32,
        _oppdatering: Faktaoppdatering,
    ) -> Result<(), anyhow::Error> {
        self.push(Skriving::Ok);
        Ok(())
    }

    async fn fullfor_poll(
        &self,
        _operasjon_id: OperasjonId,
        _attempt_no: i32,
        _oppdatering: Faktaoppdatering,
        neste_forsok_at: DateTime<Utc>,
    ) -> Result<(), anyhow::Error> {
        self.push(Skriving::Poll { neste_forsok_at });
        Ok(())
    }

    async fn marker_retry(
        &self,
        _operasjon_id: OperasjonId,
        _attempt_no: i32,
        detalj: &str,
        _neste_forsok_at: DateTime<Utc>,
    ) -> Result<(), anyhow::Error> {
        self.push(Skriving::Retry {
            detalj: detalj.to_string(),
        });
        Ok(())
    }

    async fn marker_feilet(
        &self,
        _operasjon_id: OperasjonId,
        _attempt_no: i32,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        self.push(Skriving::Feilet {
            detalj: detalj.to_string(),
        });
        Ok(())
    }

    async fn marker_blokkert(
        &self,
        _operasjon_id: OperasjonId,
        _attempt_no: Option<i32>,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        self.push(Skriving::Blokkert {
            detalj: detalj.to_string(),
        });
        Ok(())
    }

    async fn gjenopprett_etter_restart(&self) -> Result<Gjenoppretting, anyhow::Error> {
        Ok(Gjenoppretting::default())
    }

    async fn hent_krever_avklaring(&self) -> Result<Vec<Operasjon>, anyhow::Error> {
        Ok(Vec::new())
    }

    async fn marker_avklaring_varslet(
        &self,
        _operasjon_id: OperasjonId,
    ) -> Result<(), anyhow::Error> {
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
            command_id: Uuid::from_u128(42),
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

    async fn hent_command_outcome(
        &self,
        _command_id: Uuid,
    ) -> Result<CommandOutcome, anyhow::Error> {
        Ok(*self.outcome.lock().unwrap())
    }

    async fn hent_varselkandidater(
        &self,
        _eldre_enn: DateTime<Utc>,
    ) -> Result<Vec<Operasjon>, anyhow::Error> {
        Ok(Vec::new())
    }

    async fn marker_varslet(&self, _operasjon_id: OperasjonId) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[derive(Clone)]
struct FakeFaktaRepository {
    facts: SakMedBarn,
    journalpost_attributter: Option<JournalpostAttributter>,
}

#[async_trait]
impl FaktaRepository for FakeFaktaRepository {
    async fn hent_sak_med_barn(
        &self,
        _sak_id: SkuffenSakId,
    ) -> Result<Option<SakMedBarn>, anyhow::Error> {
        Ok(Some(self.facts.clone()))
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
        Ok(self.journalpost_attributter.clone())
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

/// Argumentene et avskrivingskall ble gjort med: journalpostnummer,
/// avskrivingsmåte og avskrevet av.
type Avskrivingsargumenter = (i32, Option<String>, Option<String>);

/// Gateway som feiler med nøyaktig den feilen testen ber om.
#[derive(Clone)]
struct FeilendeArkivGateway {
    feil: EksekveringFeil,
    kall: Arc<Mutex<usize>>,
    avskrivingskall: Arc<Mutex<Option<Avskrivingsargumenter>>>,
}

impl FeilendeArkivGateway {
    fn new(feil: EksekveringFeil) -> Self {
        Self {
            feil,
            kall: Arc::new(Mutex::new(0)),
            avskrivingskall: Arc::new(Mutex::new(None)),
        }
    }

    fn antall_kall(&self) -> usize {
        *self.kall.lock().unwrap()
    }

    fn feil<T>(&self) -> Result<T, EksekveringFeil> {
        *self.kall.lock().unwrap() += 1;
        Err(self.feil.clone())
    }

    fn avskrivingskall(&self) -> Option<Avskrivingsargumenter> {
        self.avskrivingskall.lock().unwrap().clone()
    }
}

#[async_trait]
impl ArkivGateway for FeilendeArkivGateway {
    async fn opprett_sak(
        &self,
        _attributter: &SakAttributter,
    ) -> Result<OpprettSakResultat, EksekveringFeil> {
        self.feil()
    }

    async fn opprett_journalpost(
        &self,
        _saksnummer: &str,
        _journalpost: &JournalpostAttributter,
        _hoveddokument: &DokumentAttributter,
    ) -> Result<OpprettJournalpostResultat, EksekveringFeil> {
        self.feil()
    }

    async fn legg_til_vedlegg(
        &self,
        _journalpost_id: i32,
        _vedlegg: &DokumentAttributter,
    ) -> Result<Option<i32>, EksekveringFeil> {
        self.feil()
    }

    async fn sett_journalpost_status(
        &self,
        _journalpost_id: i32,
        _status: Journalstatus,
    ) -> Result<(), EksekveringFeil> {
        self.feil()
    }

    async fn avskriv_journalpost(
        &self,
        journalpost_id: i32,
        kildesystem: Option<&str>,
        merknad: Option<&str>,
    ) -> Result<(), EksekveringFeil> {
        *self.avskrivingskall.lock().unwrap() = Some((
            journalpost_id,
            kildesystem.map(str::to_string),
            merknad.map(str::to_string),
        ));
        self.feil()
    }

    async fn hent_journalstatus(
        &self,
        _journalpost_id: i32,
    ) -> Result<ObservertJournalstatus, EksekveringFeil> {
        self.feil()
    }

    async fn avslutt_sak(&self, _saksnummer: &str) -> Result<(), EksekveringFeil> {
        self.feil()
    }

    async fn sett_saksansvarlig(
        &self,
        _saksnummer: &str,
        _saksbehandler_id: &str,
        _saksbehandler_enhet: &str,
    ) -> Result<(), EksekveringFeil> {
        self.feil()
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
        unreachable!("render brukes ikke i disse testene")
    }
}

#[derive(Clone, Default)]
struct FakeStatusPublisher {
    operasjonstatuser: Arc<Mutex<Vec<Operasjonstatus>>>,
}

#[async_trait]
impl StatusPublisher for FakeStatusPublisher {
    async fn publiser_command_status(
        &self,
        _status: domain::eksekvering::typer::CommandStatus,
    ) -> Result<(), anyhow::Error> {
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

const SAKSNUMMER: &str = "2026/000123";

fn sak_id() -> SkuffenSakId {
    SkuffenSakId::from(Uuid::from_u128(1))
}

/// En sak der `SettSaksansvarlig` er `Utfor`: saksnummer finnes, ønsket
/// saksansvarlig er satt, og nåværende er en annen.
fn facts_klar_for_sett_saksansvarlig() -> SakMedBarn {
    SakMedBarn {
        sak_id: sak_id(),
        tilstand: SakTilstand::Opprettet,
        arkiv_id: Some(SAKSNUMMER.to_string()),
        oensket_saksansvarlig: Some(Saksansvarlig {
            saksbehandler_id: "Z12345".to_string(),
            enhet: "MT-1".to_string(),
        }),
        naavaerende_saksansvarlig: None,
        journalposter: Vec::new(),
    }
}

fn operasjon() -> Operasjon {
    Operasjon {
        operasjon_id: OperasjonId(Uuid::from_u128(2)),
        operasjonstype: Operasjonstype::SettSaksansvarlig,
        entitet_id: EntitetId::Sak(sak_id()),
        sak_id: sak_id(),
    }
}

struct Oppsett {
    operasjon_repo: FakeOperasjonRepository,
    gateway: FeilendeArkivGateway,
    publisher: FakeStatusPublisher,
    service: EksekverOperasjonService,
}

fn oppsett(feil: EksekveringFeil) -> Oppsett {
    let operasjon_repo = FakeOperasjonRepository::default();
    let gateway = FeilendeArkivGateway::new(feil);
    let publisher = FakeStatusPublisher::default();
    let fakta = FakeFaktaRepository {
        facts: facts_klar_for_sett_saksansvarlig(),
        journalpost_attributter: None,
    };

    let service = EksekverOperasjonService::new(
        Box::new(operasjon_repo.clone()),
        Box::new(fakta),
        Box::new(gateway.clone()),
        Box::new(UbruktRenderOperasjon),
        Box::new(publisher.clone()),
        "test-executor",
        Duration::from_secs(60),
    );

    Oppsett {
        operasjon_repo,
        gateway,
        publisher,
        service,
    }
}

fn oppsett_for_avskriving(feil: EksekveringFeil) -> Oppsett {
    let journalpost_id = SkuffenJournalpostId::from(Uuid::from_u128(3));
    let operasjon_repo = FakeOperasjonRepository::default();
    let gateway = FeilendeArkivGateway::new(feil);
    let publisher = FakeStatusPublisher::default();
    let fakta = FakeFaktaRepository {
        facts: SakMedBarn {
            sak_id: sak_id(),
            tilstand: SakTilstand::Opprettet,
            arkiv_id: Some(SAKSNUMMER.to_string()),
            oensket_saksansvarlig: None,
            naavaerende_saksansvarlig: None,
            journalposter: vec![JournalpostMedDokumenter {
                journalpost_id,
                tilstand: JournalpostTilstand::Journalfoert,
                arkiv_id: Some("123".to_string()),
                journalposttype: JournalpostType::Inngaende,
                med_utsending: false,
                dokumenter: Vec::new(),
            }],
        },
        journalpost_attributter: Some(JournalpostAttributter {
            client_reference: Uuid::from_u128(4),
            tittel: "Test".to_string(),
            dokument_dato: "2026-08-27".to_string(),
            journalposttype: JournalpostType::Inngaende,
            med_utsending: false,
            saksbehandler_id: "Z12345".to_string(),
            saksbehandler_enhet: "MT-1".to_string(),
            tilgang: Tilgang::default(),
            korrespondanseparter: Korrespondanseparter::Ingen,
            kildesystem: Some("fagsystem & arkiv".to_string()),
        }),
    };
    let service = EksekverOperasjonService::new(
        Box::new(operasjon_repo.clone()),
        Box::new(fakta),
        Box::new(gateway.clone()),
        Box::new(UbruktRenderOperasjon),
        Box::new(publisher.clone()),
        "test-executor",
        Duration::from_secs(60),
    );

    Oppsett {
        operasjon_repo,
        gateway,
        publisher,
        service,
    }
}

fn sikri_irrecoverable() -> EksekveringFeil {
    EksekveringFeil::irrecoverable(
        "sikri_unknown_user",
        "Ugyldig saksbehandler/systembruker: brukeren finnes ikke i ePhorte.",
        StatusErrorCode::InvalidRequest,
    )
}

fn sikri_recoverable() -> EksekveringFeil {
    EksekveringFeil::recoverable(
        "sikri_upstream_unavailable",
        "Sikri/Elements er midlertidig utilgjengelig. Prøv igjen senere.",
        StatusErrorCode::TemporaryUnavailable,
    )
}

// ---------------------------------------------------------------------------
// Tester
// ---------------------------------------------------------------------------

#[tokio::test]
async fn irrecoverable_arkivfeil_gir_terminal_feilet_ikke_retry() {
    // Dette er defekten SKU-0016 R6 beskriver bort: før fiksen ble enhver
    // gateway-feil mappet til recoverable, og operasjonen gikk i evig retry.
    let Oppsett {
        operasjon_repo,
        gateway,
        publisher,
        service,
    } = oppsett(sikri_irrecoverable());

    service.execute(operasjon()).await.unwrap();

    assert_eq!(gateway.antall_kall(), 1);

    let skrivinger = operasjon_repo.skrivinger();
    assert!(
        matches!(skrivinger.last(), Some(Skriving::Feilet { .. })),
        "forventet terminal feilet, fikk {skrivinger:?}"
    );
    assert!(
        !skrivinger
            .iter()
            .any(|s| matches!(s, Skriving::Retry { .. })),
        "irrecoverable skal aldri planlegge nytt forsøk, fikk {skrivinger:?}"
    );

    // siste_detalj er den stabile koden, ikke fritekst.
    match skrivinger.last().unwrap() {
        Skriving::Feilet { detalj } => assert_eq!(detalj, "sikri_unknown_user"),
        annet => panic!("forventet Feilet, fikk {annet:?}"),
    }

    let statuser = publisher.operasjonstatuser.lock().unwrap();
    let siste = statuser.last().expect("ingen operasjonstatus publisert");
    assert_eq!(siste.hendelse, Operasjonshendelse::Feilet);
    assert!(siste.terminal);
    // Klienten får den faktiske grunnen, ikke «Operasjonen kunne ikke
    // fullføres.» for alt.
    assert_eq!(
        siste.melding,
        "Ugyldig saksbehandler/systembruker: brukeren finnes ikke i ePhorte."
    );
    assert_eq!(siste.error_code, Some(StatusErrorCode::InvalidRequest));
}

#[tokio::test]
async fn recoverable_arkivfeil_gir_retry_ikke_terminal_feil() {
    let Oppsett {
        operasjon_repo,
        gateway,
        publisher,
        service,
    } = oppsett(sikri_recoverable());

    service.execute(operasjon()).await.unwrap();

    assert_eq!(gateway.antall_kall(), 1);

    let skrivinger = operasjon_repo.skrivinger();
    assert!(
        matches!(skrivinger.last(), Some(Skriving::Retry { .. })),
        "forventet retry_venter, fikk {skrivinger:?}"
    );
    assert!(
        !skrivinger
            .iter()
            .any(|s| matches!(s, Skriving::Feilet { .. })),
        "recoverable skal aldri gå terminalt, fikk {skrivinger:?}"
    );

    match skrivinger.last().unwrap() {
        Skriving::Retry { detalj } => assert_eq!(detalj, "sikri_upstream_unavailable"),
        annet => panic!("forventet Retry, fikk {annet:?}"),
    }

    let statuser = publisher.operasjonstatuser.lock().unwrap();
    let siste = statuser.last().expect("ingen operasjonstatus publisert");
    assert_eq!(siste.hendelse, Operasjonshendelse::ForsokFeilet);
    assert!(!siste.terminal, "nytt forsøk kommer, så ikke terminal");
    assert_eq!(
        siste.error_code,
        Some(StatusErrorCode::TemporaryUnavailable)
    );
}

#[tokio::test]
async fn avskriving_bruker_sikri_id_og_materialisert_kildesystem_uten_oppdiktet_merknad() {
    let Oppsett {
        gateway, service, ..
    } = oppsett_for_avskriving(sikri_irrecoverable());
    let journalpost_id = SkuffenJournalpostId::from(Uuid::from_u128(3));
    let operasjon = Operasjon {
        operasjon_id: OperasjonId(Uuid::from_u128(5)),
        operasjonstype: Operasjonstype::Avskriv,
        entitet_id: EntitetId::Journalpost(journalpost_id),
        sak_id: sak_id(),
    };

    service.execute(operasjon).await.unwrap();

    assert_eq!(
        gateway.avskrivingskall(),
        Some((123, Some("fagsystem & arkiv".to_string()), None))
    );
}

#[tokio::test]
async fn skriveoperasjon_commiter_sendt_for_arkivkallet() {
    // At-most-once-grensen (SKU-0016 R4) skal ligge foran kallet også når
    // kallet feiler terminalt.
    let Oppsett {
        operasjon_repo,
        service,
        ..
    } = oppsett(sikri_irrecoverable());

    service.execute(operasjon()).await.unwrap();

    let skrivinger = operasjon_repo.skrivinger();
    assert_eq!(skrivinger[0], Skriving::Kjorer);
    assert_eq!(skrivinger[1], Skriving::Sendt);
}

#[tokio::test]
async fn intern_feil_lekker_ikke_detaljen_til_klienten() {
    let Oppsett {
        operasjon_repo,
        publisher,
        service,
        ..
    } = oppsett(
        EksekveringFeil::intern("intern_uventet_tilstand")
            .med_intern_detalj("column \"foo\" does not exist"),
    );

    service.execute(operasjon()).await.unwrap();

    // Detaljen er bevart der driften trenger den ...
    match operasjon_repo.skrivinger().last().unwrap() {
        Skriving::Feilet { detalj } => assert_eq!(
            detalj,
            "intern_uventet_tilstand column \"foo\" does not exist"
        ),
        annet => panic!("forventet Feilet, fikk {annet:?}"),
    }

    // ... men klienten ser ingenting av Skuffens innside.
    let statuser = publisher.operasjonstatuser.lock().unwrap();
    let siste = statuser.last().unwrap();
    assert_eq!(siste.melding, "Intern feil i behandlingen.");
    assert!(!siste.melding.contains("column"));
    assert_eq!(siste.error_code, Some(StatusErrorCode::ProcessingFailed));
}
