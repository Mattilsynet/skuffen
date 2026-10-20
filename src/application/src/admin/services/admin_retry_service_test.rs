use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::operasjon::{EntitetType, Operasjonsstatus};
use domain::eksekvering::typer::{
    CommandEvent, CommandStatus, CommandTypeCode, Operasjonstatus, StatusErrorCode,
};
use uuid::Uuid;

use crate::admin::model::AdminSakNokkel;
use crate::admin::ports::admin_retry_port::{
    AdminRetryRepository, FrigivelseFeil, OriginalCommandKilde, RetryGrunnlag,
};
use crate::admin::services::admin_retry_service::{AdminRetryError, AdminRetryService};
use crate::command::ports::command_port::{CommandRepository, Mottaksresultat};
use crate::command::ports::command_state_port::{
    ArkivSakTilstand, ArkivSakTilstandError, ArkivSakTilstandRepository,
};
use crate::command::ports::entitet_port::{Entitet, EntitetRepository, NyEntitet};
use crate::command::ports::status_publisher_port::StatusPublisher;
use crate::command::ports::validated_command_dispatcher_port::ValidatedCommandDispatcher;
use crate::command::services::validate_command::ValidateCommandService;
use crate::command::{Command, CommandEnvelope, SakKey, test_fixtures};

const SAKSNUMMER: &str = "2026/117";

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FakeRetryRepository {
    saker: Vec<(AdminSakNokkel, SkuffenSakId)>,
    sak_kall: Mutex<Vec<(AdminSakNokkel, usize)>>,
    oppslag_feiler: bool,
    grunnlag: Mutex<Vec<RetryGrunnlag>>,
    grunnlag_kall: Mutex<Vec<Vec<Uuid>>>,
    uavklarte_saker: Vec<SkuffenSakId>,
    frigi_feil: Mutex<Option<FrigivelseFeil>>,
    frigi_kall: Mutex<Vec<(Vec<SkuffenSakId>, Vec<Uuid>)>>,
}

#[async_trait]
impl AdminRetryRepository for FakeRetryRepository {
    async fn hent_commands_for_sak(
        &self,
        key: AdminSakNokkel,
        grense: usize,
    ) -> anyhow::Result<Option<Vec<Uuid>>> {
        self.sak_kall.lock().unwrap().push((key.clone(), grense));
        if self.oppslag_feiler {
            anyhow::bail!("saksoppslag feilet");
        }
        Ok(self
            .saker
            .iter()
            .find(|(nokkel, _)| *nokkel == key)
            .map(|(_, sak_id)| {
                self.grunnlag
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|g| g.sak_id == Some(*sak_id))
                    .take(grense)
                    .map(|g| g.command_id)
                    .collect()
            }))
    }

    async fn hent_grunnlag(&self, command_ids: &[Uuid]) -> anyhow::Result<Vec<RetryGrunnlag>> {
        self.grunnlag_kall
            .lock()
            .unwrap()
            .push(command_ids.to_vec());
        Ok(self
            .grunnlag
            .lock()
            .unwrap()
            .iter()
            .filter(|g| command_ids.contains(&g.command_id))
            .cloned()
            .collect())
    }

    async fn frigi(
        &self,
        saker: &[SkuffenSakId],
        command_ids: &[Uuid],
    ) -> Result<(), FrigivelseFeil> {
        self.frigi_kall
            .lock()
            .unwrap()
            .push((saker.to_vec(), command_ids.to_vec()));
        if let Some(feil) = self.frigi_feil.lock().unwrap().take() {
            return Err(feil);
        }
        let sperret: Vec<SkuffenSakId> = saker
            .iter()
            .filter(|sak| self.uavklarte_saker.contains(sak))
            .copied()
            .collect();
        if !sperret.is_empty() {
            return Err(FrigivelseFeil::SakHarUavklartUtfall(sperret));
        }
        Ok(())
    }
}

#[derive(Default)]
struct FakeOriginaler {
    originaler: HashMap<Uuid, CommandEnvelope<Command>>,
    lest: Mutex<Vec<Uuid>>,
}

#[async_trait]
impl OriginalCommandKilde for FakeOriginaler {
    async fn hent(
        &self,
        command_id: Uuid,
        _command_type: CommandTypeCode,
    ) -> anyhow::Result<Option<CommandEnvelope<Command>>> {
        self.lest.lock().unwrap().push(command_id);
        Ok(self.originaler.get(&command_id).cloned())
    }
}

#[derive(Default)]
struct FakeCommandRepository {
    saker: Mutex<HashMap<Uuid, SkuffenSakId>>,
}

#[async_trait]
impl CommandRepository for FakeCommandRepository {
    async fn registrer_mottatt(
        &self,
        _envelope: &CommandEnvelope<Command>,
    ) -> anyhow::Result<Mottaksresultat> {
        unreachable!("admin retry går ikke gjennom mottak")
    }

    async fn marker_dispatchet(&self, _command_id: Uuid) -> anyhow::Result<()> {
        unreachable!("admin retry nullstiller ikke mottaksjournalen")
    }

    async fn knytt_sak(&self, command_id: Uuid, sak_id: SkuffenSakId) -> anyhow::Result<()> {
        self.saker.lock().unwrap().insert(command_id, sak_id);
        Ok(())
    }
}

#[derive(Default)]
struct FakeEntitetRepository {
    arkiv: HashMap<String, Uuid>,
}

#[async_trait]
impl EntitetRepository for FakeEntitetRepository {
    async fn registrer(&self, _entitet: NyEntitet) -> anyhow::Result<Uuid> {
        unreachable!("admin retry oppretter ingen identitet")
    }

    async fn hent_for_client_reference(
        &self,
        _client_reference: Uuid,
    ) -> anyhow::Result<Option<Entitet>> {
        Ok(None)
    }

    async fn hent_for_arkiv_id(
        &self,
        entitet_type: EntitetType,
        arkiv_id: &str,
    ) -> anyhow::Result<Option<Entitet>> {
        Ok(self.arkiv.get(arkiv_id).map(|skuffen_id| Entitet {
            skuffen_id: *skuffen_id,
            entitet_type,
            client_reference: None,
            arkiv_id: Some(arkiv_id.to_string()),
        }))
    }

    async fn hent_eller_opprett_for_arkiv_id(
        &self,
        _entitet_type: EntitetType,
        _arkiv_id: &str,
    ) -> anyhow::Result<Uuid> {
        unreachable!("admin retry oppretter ingen identitet")
    }

    async fn hent_arkiv_id(&self, _skuffen_id: Uuid) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

#[derive(Clone, Copy)]
enum Arkivsvar {
    Aapen,
    Avsluttet,
    Utilgjengelig,
}

struct FakeArkiv(Arkivsvar);

#[async_trait]
impl ArkivSakTilstandRepository for FakeArkiv {
    async fn hent_sak_tilstand_fra_arkivet(
        &self,
        _saksnummer: &str,
    ) -> Result<ArkivSakTilstand, ArkivSakTilstandError> {
        match self.0 {
            Arkivsvar::Aapen => Ok(ArkivSakTilstand { avsluttet: false }),
            Arkivsvar::Avsluttet => Ok(ArkivSakTilstand { avsluttet: true }),
            Arkivsvar::Utilgjengelig => Err(ArkivSakTilstandError::utilgjengelig()),
        }
    }
}

#[derive(Clone, Default)]
struct Observert {
    dispatch_feiler_for: Arc<Mutex<Option<Uuid>>>,
    status_feiler_for: Arc<Mutex<Option<Uuid>>>,
    dispatch_forsok: Arc<Mutex<Vec<Uuid>>>,
    status_forsok: Arc<Mutex<Vec<Uuid>>>,
    videresendt: Arc<Mutex<Vec<Uuid>>>,
    status: Arc<Mutex<Vec<CommandStatus>>>,
}

#[async_trait]
impl ValidatedCommandDispatcher for Observert {
    async fn dispatch_validated(&self, command: &CommandEnvelope<Command>) -> anyhow::Result<()> {
        self.dispatch_forsok
            .lock()
            .unwrap()
            .push(command.command_id);
        if *self.dispatch_feiler_for.lock().unwrap() == Some(command.command_id) {
            anyhow::bail!("dispatch feilet");
        }
        self.videresendt.lock().unwrap().push(command.command_id);
        Ok(())
    }
}

#[async_trait]
impl StatusPublisher for Observert {
    async fn publiser_command_status(&self, status: CommandStatus) -> anyhow::Result<()> {
        self.status_forsok.lock().unwrap().push(status.command_id);
        if *self.status_feiler_for.lock().unwrap() == Some(status.command_id) {
            anyhow::bail!("statuspublisering feilet");
        }
        self.status.lock().unwrap().push(status);
        Ok(())
    }

    async fn publiser_operasjonstatus(&self, _status: Operasjonstatus) -> anyhow::Result<()> {
        Ok(())
    }
}

struct Oppsett {
    repository: Arc<FakeRetryRepository>,
    originaler: Arc<FakeOriginaler>,
    command_repo: Arc<FakeCommandRepository>,
    observert: Observert,
    service: AdminRetryService,
}

fn oppsett(
    repository: FakeRetryRepository,
    originaler: FakeOriginaler,
    entiteter: FakeEntitetRepository,
    arkiv: Arkivsvar,
) -> Oppsett {
    let repository = Arc::new(repository);
    let originaler = Arc::new(originaler);
    let command_repo = Arc::new(FakeCommandRepository::default());
    let observert = Observert::default();
    let validator = ValidateCommandService::new(
        Box::new(FakeArkiv(arkiv)),
        Box::new(FakeEntitetRepository::default()),
        Box::new(observert.clone()),
        Box::new(observert.clone()),
    );
    let service = AdminRetryService::new(
        repository.clone(),
        originaler.clone(),
        command_repo.clone(),
        Arc::new(entiteter),
        validator,
    );
    Oppsett {
        repository,
        originaler,
        command_repo,
        observert,
        service,
    }
}

fn avvist_grunnlag(command_id: Uuid, sak_id: Option<SkuffenSakId>) -> RetryGrunnlag {
    RetryGrunnlag {
        command_id,
        command_type: CommandTypeCode::AvsluttSak,
        sak_id,
        dekomponert: false,
        forkastet: false,
        operasjoner: Vec::new(),
    }
}

fn avslutt_original(command_id: Uuid) -> CommandEnvelope<Command> {
    CommandEnvelope::new(
        command_id,
        test_fixtures::avslutt_sak(SakKey::ArkivId(SAKSNUMMER.to_string())),
    )
}

fn med_original(command_id: Uuid) -> FakeOriginaler {
    FakeOriginaler {
        originaler: HashMap::from([(command_id, avslutt_original(command_id))]),
        ..Default::default()
    }
}

fn med_grunnlag(grunnlag: Vec<RetryGrunnlag>) -> FakeRetryRepository {
    FakeRetryRepository {
        grunnlag: Mutex::new(grunnlag),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Tester
// ---------------------------------------------------------------------------

#[tokio::test]
async fn avvist_kommando_valideres_paa_nytt_fra_originalen_etter_retting() {
    let command_id = Uuid::new_v4();
    let sak_id = SkuffenSakId(Uuid::new_v4());
    let t = oppsett(
        med_grunnlag(vec![avvist_grunnlag(command_id, Some(sak_id))]),
        med_original(command_id),
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );

    t.service.handle(vec![command_id]).await.unwrap();
    assert_eq!(*t.observert.videresendt.lock().unwrap(), vec![command_id]);
    let status = t.observert.status.lock().unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(status[0].hendelse, CommandEvent::Validert);
    assert_eq!(t.repository.frigi_kall.lock().unwrap()[0].0, vec![sak_id]);
}

#[tokio::test]
async fn samme_feil_igjen_er_et_gyldig_utfall() {
    let command_id = Uuid::new_v4();
    let t = oppsett(
        med_grunnlag(vec![avvist_grunnlag(command_id, None)]),
        med_original(command_id),
        FakeEntitetRepository::default(),
        Arkivsvar::Avsluttet,
    );

    t.service.handle(vec![command_id]).await.unwrap();
    assert!(t.observert.videresendt.lock().unwrap().is_empty());
    assert_eq!(
        t.observert.status.lock().unwrap()[0].hendelse,
        CommandEvent::Avvist
    );
    assert_eq!(
        t.observert.status.lock().unwrap()[0].error_code,
        Some(StatusErrorCode::Conflict)
    );
}

#[tokio::test]
async fn recoverable_validering_gir_ok_uten_dispatch_eller_status() {
    let command_id = Uuid::new_v4();
    let t = oppsett(
        med_grunnlag(vec![avvist_grunnlag(command_id, None)]),
        med_original(command_id),
        FakeEntitetRepository::default(),
        Arkivsvar::Utilgjengelig,
    );

    t.service.handle(vec![command_id]).await.unwrap();

    assert!(t.observert.videresendt.lock().unwrap().is_empty());
    assert!(t.observert.dispatch_forsok.lock().unwrap().is_empty());
    assert!(t.observert.status_forsok.lock().unwrap().is_empty());
    assert!(t.observert.status.lock().unwrap().is_empty());
}

#[tokio::test]
async fn eldre_kommando_uten_saksreferanse_kobles_fra_originalen() {
    let command_id = Uuid::new_v4();
    let sak_uuid = Uuid::new_v4();
    let t = oppsett(
        med_grunnlag(vec![avvist_grunnlag(command_id, None)]),
        med_original(command_id),
        FakeEntitetRepository {
            arkiv: HashMap::from([(SAKSNUMMER.to_string(), sak_uuid)]),
        },
        Arkivsvar::Aapen,
    );

    t.service.handle(vec![command_id]).await.unwrap();

    assert_eq!(
        t.command_repo.saker.lock().unwrap().get(&command_id),
        Some(&SkuffenSakId(sak_uuid))
    );
    assert_eq!(
        t.repository.frigi_kall.lock().unwrap()[0].0,
        vec![SkuffenSakId(sak_uuid)],
        "saken kontrolleres for uavklart utfall selv om koblingen manglet"
    );
}

#[tokio::test]
async fn uavklart_utfall_paa_saken_avviser_hele_foresporselen_uten_revalidering() {
    let avvist = Uuid::new_v4();
    let feilet = Uuid::new_v4();
    let sak_id = SkuffenSakId(Uuid::new_v4());
    let mut repository = med_grunnlag(vec![
        avvist_grunnlag(avvist, Some(sak_id)),
        RetryGrunnlag {
            command_id: feilet,
            command_type: CommandTypeCode::SettSaksansvarlig,
            sak_id: Some(sak_id),
            dekomponert: true,
            forkastet: false,
            operasjoner: vec![Operasjonsstatus::Feilet],
        },
    ]);
    repository.uavklarte_saker = vec![sak_id];
    let t = oppsett(
        repository,
        med_original(avvist),
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );

    let feil = t.service.handle(vec![avvist, feilet]).await.unwrap_err();

    assert!(matches!(feil, AdminRetryError::SakHarUavklartUtfall(saker) if saker == vec![sak_id]));
    assert!(t.observert.videresendt.lock().unwrap().is_empty());
    assert!(t.observert.status.lock().unwrap().is_empty());
}

#[tokio::test]
async fn manglende_original_avviser_hele_foresporselen_foer_frigivelse_og_validering() {
    let command_id = Uuid::new_v4();
    let t = oppsett(
        med_grunnlag(vec![avvist_grunnlag(command_id, None)]),
        FakeOriginaler::default(),
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );

    let feil = t.service.handle(vec![command_id]).await.unwrap_err();

    assert!(
        matches!(feil, AdminRetryError::OriginalIkkeTilgjengelig(ids) if ids == vec![command_id])
    );
    assert!(t.repository.frigi_kall.lock().unwrap().is_empty());
}

#[tokio::test]
async fn ukjent_kommando_avviser_hele_foresporselen() {
    let kjent = Uuid::new_v4();
    let ukjent = Uuid::new_v4();
    let t = oppsett(
        med_grunnlag(vec![avvist_grunnlag(kjent, None)]),
        med_original(kjent),
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );

    let feil = t.service.handle(vec![kjent, ukjent]).await.unwrap_err();

    assert!(matches!(feil, AdminRetryError::CommandNotFound(ids) if ids == vec![ukjent]));
    assert!(t.originaler.lest.lock().unwrap().is_empty());
    assert!(t.repository.frigi_kall.lock().unwrap().is_empty());
}

#[tokio::test]
async fn forkastet_kommando_avviser_hele_foresporselen() {
    let kjent = Uuid::new_v4();
    let forkastet = Uuid::new_v4();
    let t = oppsett(
        med_grunnlag(vec![
            avvist_grunnlag(kjent, None),
            RetryGrunnlag {
                forkastet: true,
                ..avvist_grunnlag(forkastet, None)
            },
        ]),
        med_original(kjent),
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );

    let feil = t.service.handle(vec![kjent, forkastet]).await.unwrap_err();

    assert!(matches!(feil, AdminRetryError::Forkastet(ids) if ids == vec![forkastet]));
    assert!(t.originaler.lest.lock().unwrap().is_empty());
    assert!(t.repository.frigi_kall.lock().unwrap().is_empty());
}

#[tokio::test]
async fn fullfort_kommando_revalideres_ikke() {
    let command_id = Uuid::new_v4();
    let t = oppsett(
        med_grunnlag(vec![RetryGrunnlag {
            command_id,
            command_type: CommandTypeCode::AvsluttSak,
            sak_id: Some(SkuffenSakId(Uuid::new_v4())),
            dekomponert: true,
            forkastet: false,
            operasjoner: vec![Operasjonsstatus::Ok],
        }]),
        med_original(command_id),
        FakeEntitetRepository::default(),
        Arkivsvar::Avsluttet,
    );

    t.service.handle(vec![command_id]).await.unwrap();
    assert!(
        t.originaler.lest.lock().unwrap().is_empty(),
        "en vellykket AvsluttSak ville blitt avvist av sin egen lukking"
    );
    assert!(t.observert.status.lock().unwrap().is_empty());
    assert!(t.observert.videresendt.lock().unwrap().is_empty());
    assert!(t.repository.frigi_kall.lock().unwrap()[0].1.is_empty());
}

#[tokio::test]
async fn feilede_operasjoner_frigis_til_executoren() {
    let command_id = Uuid::new_v4();
    let sak_id = SkuffenSakId(Uuid::new_v4());
    let repository = med_grunnlag(vec![RetryGrunnlag {
        command_id,
        command_type: CommandTypeCode::OpprettInterntNotatJournalpost,
        sak_id: Some(sak_id),
        dekomponert: true,
        forkastet: false,
        operasjoner: vec![Operasjonsstatus::Ok, Operasjonsstatus::Feilet],
    }]);
    let t = oppsett(
        repository,
        FakeOriginaler::default(),
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );

    t.service.handle(vec![command_id]).await.unwrap();
    assert_eq!(
        *t.repository.frigi_kall.lock().unwrap(),
        vec![(vec![sak_id], vec![command_id])]
    );
    assert!(t.originaler.lest.lock().unwrap().is_empty());
    assert!(t.observert.videresendt.lock().unwrap().is_empty());
}

#[tokio::test]
async fn duplikater_behandles_en_gang_i_innsendt_rekkefolge() {
    let forste = Uuid::new_v4();
    let andre = Uuid::new_v4();
    let t = oppsett(
        med_grunnlag(vec![
            avvist_grunnlag(andre, None),
            avvist_grunnlag(forste, None),
        ]),
        FakeOriginaler {
            originaler: HashMap::from([
                (forste, avslutt_original(forste)),
                (andre, avslutt_original(andre)),
            ]),
            ..Default::default()
        },
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );

    t.service.handle(vec![forste, andre, forste]).await.unwrap();
    assert_eq!(
        *t.observert.videresendt.lock().unwrap(),
        vec![forste, andre]
    );
}

#[tokio::test]
async fn tom_foresporsel_avvises() {
    let t = oppsett(
        FakeRetryRepository::default(),
        FakeOriginaler::default(),
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );

    assert!(matches!(
        t.service.handle(Vec::new()).await,
        Err(AdminRetryError::TomForesporsel)
    ));
}

fn assert_ingen_retryarbeid(t: &Oppsett) {
    assert!(t.repository.grunnlag_kall.lock().unwrap().is_empty());
    assert!(t.repository.frigi_kall.lock().unwrap().is_empty());
    assert!(t.originaler.lest.lock().unwrap().is_empty());
    assert!(t.command_repo.saker.lock().unwrap().is_empty());
    assert!(t.observert.videresendt.lock().unwrap().is_empty());
    assert!(t.observert.status.lock().unwrap().is_empty());
}

#[tokio::test]
async fn dispatchfeil_paa_andre_kommando_stopper_tredje_med_command_id_i_feilen() {
    let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let t = oppsett(
        med_grunnlag(ids.iter().map(|id| avvist_grunnlag(*id, None)).collect()),
        FakeOriginaler {
            originaler: ids.iter().map(|id| (*id, avslutt_original(*id))).collect(),
            ..Default::default()
        },
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );
    *t.observert.dispatch_feiler_for.lock().unwrap() = Some(ids[1]);

    let feil = t.service.handle(ids.to_vec()).await.unwrap_err();

    let AdminRetryError::Teknisk(feil) = feil else {
        panic!("forventet teknisk feil");
    };
    assert_eq!(
        feil.to_string(),
        format!("admin retry: command_id={}", ids[1])
    );
    assert_eq!(feil.root_cause().to_string(), "dispatch feilet");
    assert_eq!(*t.observert.dispatch_forsok.lock().unwrap(), ids[..2]);
    assert_eq!(*t.observert.videresendt.lock().unwrap(), vec![ids[0]]);
    assert_eq!(*t.observert.status_forsok.lock().unwrap(), vec![ids[0]]);
    let statuser = t.observert.status.lock().unwrap();
    assert_eq!(statuser.len(), 1);
    assert_eq!(statuser[0].command_id, ids[0]);
    assert_eq!(statuser[0].hendelse, CommandEvent::Validert);
}

#[tokio::test]
async fn statusfeil_etter_dispatch_propageres_og_stopper_videre_revalidering() {
    let ids = [Uuid::new_v4(), Uuid::new_v4()];
    let t = oppsett(
        med_grunnlag(ids.iter().map(|id| avvist_grunnlag(*id, None)).collect()),
        FakeOriginaler {
            originaler: ids.iter().map(|id| (*id, avslutt_original(*id))).collect(),
            ..Default::default()
        },
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );
    *t.observert.status_feiler_for.lock().unwrap() = Some(ids[0]);

    let feil = t.service.handle(ids.to_vec()).await.unwrap_err();

    let AdminRetryError::Teknisk(feil) = feil else {
        panic!("forventet teknisk feil");
    };
    assert_eq!(
        feil.to_string(),
        format!("admin retry: command_id={}", ids[0])
    );
    assert_eq!(feil.root_cause().to_string(), "statuspublisering feilet");
    assert_eq!(*t.observert.dispatch_forsok.lock().unwrap(), vec![ids[0]]);
    assert_eq!(*t.observert.videresendt.lock().unwrap(), vec![ids[0]]);
    assert_eq!(*t.observert.status_forsok.lock().unwrap(), vec![ids[0]]);
    assert!(t.observert.status.lock().unwrap().is_empty());
}

#[tokio::test]
async fn frigivelsesfeil_stopper_validering_og_beholder_feiltypen() {
    for opptatt in [true, false] {
        let command_id = Uuid::new_v4();
        let mut repository = med_grunnlag(vec![avvist_grunnlag(command_id, None)]);
        repository.frigi_feil = Mutex::new(Some(if opptatt {
            FrigivelseFeil::Opptatt
        } else {
            FrigivelseFeil::Repository(anyhow::anyhow!("frigivelse feilet"))
        }));
        let t = oppsett(
            repository,
            med_original(command_id),
            FakeEntitetRepository::default(),
            Arkivsvar::Aapen,
        );

        let feil = t.service.handle(vec![command_id]).await.unwrap_err();

        if opptatt {
            assert!(matches!(feil, AdminRetryError::Opptatt));
        } else {
            assert!(
                matches!(feil, AdminRetryError::Teknisk(err) if err.to_string() == "frigivelse feilet")
            );
        }
        assert_eq!(t.repository.frigi_kall.lock().unwrap().len(), 1);
        assert!(t.observert.dispatch_forsok.lock().unwrap().is_empty());
        assert!(t.observert.status_forsok.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn manglende_original_stopper_arbeid_men_beholder_sakskobling_fra_planlegging() {
    let kjent = Uuid::new_v4();
    let mangler = Uuid::new_v4();
    let sak_id = SkuffenSakId(Uuid::new_v4());
    let t = oppsett(
        med_grunnlag(vec![
            avvist_grunnlag(kjent, None),
            avvist_grunnlag(mangler, None),
        ]),
        med_original(kjent),
        FakeEntitetRepository {
            arkiv: HashMap::from([(SAKSNUMMER.to_string(), sak_id.0)]),
        },
        Arkivsvar::Aapen,
    );

    let feil = t.service.handle(vec![kjent, mangler]).await.unwrap_err();

    assert!(matches!(feil, AdminRetryError::OriginalIkkeTilgjengelig(ids) if ids == vec![mangler]));
    assert_eq!(
        t.command_repo.saker.lock().unwrap().get(&kjent),
        Some(&sak_id)
    );
    assert!(t.repository.frigi_kall.lock().unwrap().is_empty());
    assert!(t.observert.dispatch_forsok.lock().unwrap().is_empty());
    assert!(t.observert.status_forsok.lock().unwrap().is_empty());
}

#[tokio::test]
async fn for_mange_unike_kommandoer_avvises_foer_oppslag() {
    let t = oppsett(
        FakeRetryRepository::default(),
        FakeOriginaler::default(),
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );

    let feil = t
        .service
        .handle((0..101).map(|_| Uuid::new_v4()).collect())
        .await
        .unwrap_err();

    assert!(matches!(feil, AdminRetryError::ForMange));
    assert_ingen_retryarbeid(&t);
}

#[tokio::test]
async fn kjent_sak_uten_kommandoer_gir_ok_uten_retryarbeid() {
    let sak_id = SkuffenSakId(Uuid::new_v4());
    let key = AdminSakNokkel::SkuffenId(sak_id);
    let t = oppsett(
        FakeRetryRepository {
            saker: vec![(key.clone(), sak_id)],
            ..Default::default()
        },
        FakeOriginaler::default(),
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );

    t.service.handle_sak(key.clone()).await.unwrap();

    assert_eq!(*t.repository.sak_kall.lock().unwrap(), vec![(key, 101)]);
    assert_ingen_retryarbeid(&t);
}

#[tokio::test]
async fn ukjent_sak_avvises_uten_retryarbeid() {
    let t = oppsett(
        FakeRetryRepository::default(),
        FakeOriginaler::default(),
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );
    let key = AdminSakNokkel::ClientReference(Uuid::new_v4());

    assert!(matches!(
        t.service.handle_sak(key.clone()).await,
        Err(AdminRetryError::SakNotFound)
    ));

    assert_eq!(*t.repository.sak_kall.lock().unwrap(), vec![(key, 101)]);
    assert_ingen_retryarbeid(&t);
}

#[tokio::test]
async fn feil_i_saksoppslag_returneres_uten_internt_retry() {
    let t = oppsett(
        FakeRetryRepository {
            oppslag_feiler: true,
            ..Default::default()
        },
        FakeOriginaler::default(),
        FakeEntitetRepository::default(),
        Arkivsvar::Aapen,
    );

    assert!(matches!(
        t.service
            .handle_sak(AdminSakNokkel::ArkivId(SAKSNUMMER.to_string()))
            .await,
        Err(AdminRetryError::Teknisk(_))
    ));

    assert_eq!(t.repository.sak_kall.lock().unwrap().len(), 1);
    assert_ingen_retryarbeid(&t);
}

#[tokio::test]
async fn sak_med_100_kommandoer_behandles_mens_101_avvises_foer_arbeid() {
    for antall in [100, 101] {
        let sak_id = SkuffenSakId(Uuid::new_v4());
        let key = AdminSakNokkel::SkuffenId(sak_id);
        let ids: Vec<_> = (0..antall).map(|_| Uuid::new_v4()).collect();
        let mut repository = med_grunnlag(
            ids.iter()
                .map(|id| avvist_grunnlag(*id, Some(sak_id)))
                .collect(),
        );
        repository.saker = vec![(key.clone(), sak_id)];
        let t = oppsett(
            repository,
            FakeOriginaler {
                originaler: ids.iter().map(|id| (*id, avslutt_original(*id))).collect(),
                ..Default::default()
            },
            FakeEntitetRepository::default(),
            Arkivsvar::Aapen,
        );

        let resultat = t.service.handle_sak(key.clone()).await;

        assert_eq!(*t.repository.sak_kall.lock().unwrap(), vec![(key, 101)]);
        if antall == 100 {
            resultat.unwrap();
            assert_eq!(t.observert.status.lock().unwrap().len(), 100);
            assert_eq!(*t.observert.videresendt.lock().unwrap(), ids);
            assert_eq!(*t.originaler.lest.lock().unwrap(), ids);
            assert_eq!(
                *t.repository.frigi_kall.lock().unwrap(),
                vec![(vec![sak_id], vec![])]
            );
        } else {
            assert!(matches!(resultat, Err(AdminRetryError::ForMange)));
            assert_ingen_retryarbeid(&t);
        }
    }
}

#[tokio::test]
async fn saksnokler_viderefores_og_bare_valgt_sak_gaar_gjennom_ordinaer_flyt() {
    let sak_id = SkuffenSakId(Uuid::new_v4());
    for key in [
        AdminSakNokkel::SkuffenId(sak_id),
        AdminSakNokkel::ClientReference(Uuid::new_v4()),
        AdminSakNokkel::ArkivId(SAKSNUMMER.to_string()),
    ] {
        let avvist = Uuid::new_v4();
        let feilet = Uuid::new_v4();
        let annen = Uuid::new_v4();
        let annen_sak = SkuffenSakId(Uuid::new_v4());
        let mut repository = med_grunnlag(vec![
            avvist_grunnlag(avvist, Some(sak_id)),
            avvist_grunnlag(annen, Some(annen_sak)),
            RetryGrunnlag {
                command_id: feilet,
                command_type: CommandTypeCode::SettSaksansvarlig,
                sak_id: Some(sak_id),
                dekomponert: true,
                forkastet: false,
                operasjoner: vec![Operasjonsstatus::Feilet],
            },
        ]);
        repository.saker = vec![
            (key.clone(), sak_id),
            (AdminSakNokkel::SkuffenId(annen_sak), annen_sak),
        ];
        repository.uavklarte_saker = vec![annen_sak];
        let t = oppsett(
            repository,
            med_original(avvist),
            FakeEntitetRepository::default(),
            Arkivsvar::Aapen,
        );

        t.service.handle_sak(key.clone()).await.unwrap();

        assert_eq!(*t.repository.sak_kall.lock().unwrap(), vec![(key, 101)]);
        assert_eq!(
            *t.repository.grunnlag_kall.lock().unwrap(),
            vec![vec![avvist, feilet]]
        );
        assert_eq!(
            *t.repository.frigi_kall.lock().unwrap(),
            vec![(vec![sak_id], vec![feilet])]
        );
        assert_eq!(*t.originaler.lest.lock().unwrap(), vec![avvist]);
        assert_eq!(*t.observert.videresendt.lock().unwrap(), vec![avvist]);
    }
}
