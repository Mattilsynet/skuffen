//! Listener-nivå: rutetabell, subscriptions og nedstenging. Fakes deles med
//! testene per handling.

mod read;
mod retry;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use application::admin::model::{AdminCommand, AdminSak, AdminSakNokkel};
use application::admin::ports::admin_forkast_port::{AdminForkastRepository, ForkastFeil};
use application::admin::ports::admin_read_repository::AdminReadRepository;
use application::admin::ports::admin_retry_port::{
    AdminRetryRepository, FrigivelseFeil, OriginalCommandKilde, RetryGrunnlag,
};
use application::admin::services::admin_forkast_service::AdminForkastService;
use application::admin::services::admin_read_service::AdminReadService;
use application::admin::services::admin_retry_service::AdminRetryService;
use application::command::ports::command_port::{CommandRepository, Mottaksresultat};
use application::command::ports::command_state_port::{
    ArkivSakTilstand, ArkivSakTilstandError, ArkivSakTilstandRepository,
};
use application::command::ports::entitet_port::{Entitet, EntitetRepository, NyEntitet};
use application::command::ports::status_publisher_port::StatusPublisher;
use application::command::ports::validated_command_dispatcher_port::ValidatedCommandDispatcher;
use application::command::services::validate_command::ValidateCommandService;
use application::command::{Command, CommandEnvelope};
use async_trait::async_trait;
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::operasjon::EntitetType;
use domain::eksekvering::typer::{CommandStatus, CommandTypeCode, Operasjonstatus};
use futures::{StreamExt, stream};
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::layer::SubscriberExt;
use uuid::Uuid;

use super::listener::{AdminListener, AdminServices};
use super::rute::AdminRute;
use super::svar::{COMMAND_NOT_FOUND, SAK_NOT_FOUND};
use super::transport::{AdminMessage, AdminMessageStream, AdminTransport};
use crate::http::helse::Helse;

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

struct FakeTransport {
    meldinger: Mutex<HashMap<&'static str, Vec<AdminMessage>>>,
    avsluttet: Option<&'static str>,
    subscriptions: Mutex<Vec<(&'static str, &'static str)>>,
    publisert: Mutex<Vec<(String, Value)>>,
    max_payload: usize,
    publish_feiler: bool,
    endret: Notify,
}

impl Default for FakeTransport {
    fn default() -> Self {
        Self {
            meldinger: Mutex::default(),
            avsluttet: None,
            subscriptions: Mutex::default(),
            publisert: Mutex::default(),
            max_payload: usize::MAX,
            publish_feiler: false,
            endret: Notify::new(),
        }
    }
}

impl FakeTransport {
    fn publiserte_svar(&self) -> Vec<Value> {
        self.publisert
            .lock()
            .unwrap()
            .iter()
            .map(|(_, svar)| svar.clone())
            .collect()
    }
}

#[async_trait]
impl AdminTransport for FakeTransport {
    async fn queue_subscribe(
        &self,
        subject: &'static str,
        queue_group: &'static str,
    ) -> anyhow::Result<AdminMessageStream> {
        self.subscriptions
            .lock()
            .unwrap()
            .push((subject, queue_group));
        self.endret.notify_one();
        if self.avsluttet == Some(subject) {
            return Ok(Box::pin(stream::empty()));
        }
        let meldinger = self
            .meldinger
            .lock()
            .unwrap()
            .remove(subject)
            .unwrap_or_default();
        Ok(Box::pin(stream::iter(meldinger).chain(stream::pending())))
    }

    async fn publish(&self, reply: String, payload: Vec<u8>) -> anyhow::Result<()> {
        if self.publish_feiler {
            return Err(anyhow::anyhow!("publish feilet"));
        }
        self.publisert
            .lock()
            .unwrap()
            .push((reply, serde_json::from_slice(&payload).unwrap()));
        self.endret.notify_one();
        Ok(())
    }

    fn max_payload(&self) -> usize {
        self.max_payload
    }
}

// ---------------------------------------------------------------------------
// Repositories
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum Svar<T> {
    Funnet(T),
    IkkeFunnet,
    Feil,
}

impl<T> Svar<T> {
    fn til_resultat(self) -> anyhow::Result<Option<T>> {
        match self {
            Self::Funnet(verdi) => Ok(Some(verdi)),
            Self::IkkeFunnet => Ok(None),
            Self::Feil => Err(anyhow::anyhow!("db nede")),
        }
    }
}

struct FakeReadRepository {
    command: Svar<AdminCommand>,
    sak: Svar<AdminSak>,
    kall: Mutex<Vec<&'static str>>,
}

impl FakeReadRepository {
    fn new(command: Svar<AdminCommand>, sak: Svar<AdminSak>) -> Self {
        Self {
            command,
            sak,
            kall: Mutex::default(),
        }
    }
}

impl Default for FakeReadRepository {
    fn default() -> Self {
        Self::new(Svar::IkkeFunnet, Svar::IkkeFunnet)
    }
}

#[async_trait]
impl AdminReadRepository for FakeReadRepository {
    async fn hent_command(&self, _command_id: Uuid) -> anyhow::Result<Option<AdminCommand>> {
        self.kall.lock().unwrap().push("command");
        self.command.clone().til_resultat()
    }

    async fn hent_sak(&self, _key: AdminSakNokkel) -> anyhow::Result<Option<AdminSak>> {
        self.kall.lock().unwrap().push("sak");
        self.sak.clone().til_resultat()
    }
}

#[derive(Default)]
struct FakeRetryRepository {
    sak_kall: Mutex<Vec<AdminSakNokkel>>,
    command_kall: Mutex<Vec<Vec<Uuid>>>,
    teknisk_feil: bool,
}

#[async_trait]
impl AdminRetryRepository for FakeRetryRepository {
    async fn hent_commands_for_sak(
        &self,
        key: AdminSakNokkel,
        _grense: usize,
    ) -> anyhow::Result<Option<Vec<Uuid>>> {
        self.sak_kall.lock().unwrap().push(key);
        if self.teknisk_feil {
            anyhow::bail!("syntetisk intern databasefeil med private detaljer");
        }
        Ok(Some(Vec::new()))
    }

    async fn hent_grunnlag(&self, command_ids: &[Uuid]) -> anyhow::Result<Vec<RetryGrunnlag>> {
        self.command_kall.lock().unwrap().push(command_ids.to_vec());
        if self.teknisk_feil {
            anyhow::bail!("syntetisk intern databasefeil med private detaljer");
        }
        Ok(Vec::new())
    }

    async fn frigi(
        &self,
        _saker: &[SkuffenSakId],
        _command_ids: &[Uuid],
    ) -> Result<(), FrigivelseFeil> {
        panic!("transporttestene skal stoppe før frigivelse")
    }
}

#[derive(Default)]
struct FakeForkastRepository {
    kall: Mutex<Vec<Vec<Uuid>>>,
}

#[async_trait]
impl AdminForkastRepository for FakeForkastRepository {
    async fn forkast(&self, command_ids: &[Uuid]) -> Result<(), ForkastFeil> {
        self.kall.lock().unwrap().push(command_ids.to_vec());
        Ok(())
    }
}

/// Avhengigheter retry-tjenestens validator har, men som transporttestene
/// aldri skal nå.
struct UventetKall;

#[async_trait]
impl OriginalCommandKilde for UventetKall {
    async fn hent(
        &self,
        _command_id: Uuid,
        _command_type: CommandTypeCode,
    ) -> anyhow::Result<Option<CommandEnvelope<Command>>> {
        panic!("uventet henting av original")
    }
}

#[async_trait]
impl CommandRepository for UventetKall {
    async fn registrer_mottatt(
        &self,
        _envelope: &CommandEnvelope<Command>,
    ) -> anyhow::Result<Mottaksresultat> {
        panic!("uventet registrering")
    }

    async fn marker_dispatchet(&self, _command_id: Uuid) -> anyhow::Result<()> {
        panic!("uventet dispatch")
    }

    async fn knytt_sak(&self, _command_id: Uuid, _sak_id: SkuffenSakId) -> anyhow::Result<()> {
        panic!("uventet sakskobling")
    }
}

#[async_trait]
impl EntitetRepository for UventetKall {
    async fn registrer(&self, _entitet: NyEntitet) -> anyhow::Result<Uuid> {
        panic!("uventet registrering")
    }

    async fn hent_for_client_reference(&self, _id: Uuid) -> anyhow::Result<Option<Entitet>> {
        panic!("uventet entitetsoppslag")
    }

    async fn hent_for_arkiv_id(
        &self,
        _entitet_type: EntitetType,
        _arkiv_id: &str,
    ) -> anyhow::Result<Option<Entitet>> {
        panic!("uventet entitetsoppslag")
    }

    async fn hent_eller_opprett_for_arkiv_id(
        &self,
        _entitet_type: EntitetType,
        _arkiv_id: &str,
    ) -> anyhow::Result<Uuid> {
        panic!("uventet opprettelse")
    }

    async fn hent_arkiv_id(&self, _skuffen_id: Uuid) -> anyhow::Result<Option<String>> {
        panic!("uventet entitetsoppslag")
    }
}

#[async_trait]
impl ArkivSakTilstandRepository for UventetKall {
    async fn hent_sak_tilstand_fra_arkivet(
        &self,
        _saksnummer: &str,
    ) -> Result<ArkivSakTilstand, ArkivSakTilstandError> {
        panic!("uventet arkivoppslag")
    }
}

#[async_trait]
impl ValidatedCommandDispatcher for UventetKall {
    async fn dispatch_validated(&self, _command: &CommandEnvelope<Command>) -> anyhow::Result<()> {
        panic!("uventet dispatch")
    }
}

#[async_trait]
impl StatusPublisher for UventetKall {
    async fn publiser_command_status(&self, _status: CommandStatus) -> anyhow::Result<()> {
        panic!("uventet statuspublisering")
    }

    async fn publiser_operasjonstatus(&self, _status: Operasjonstatus) -> anyhow::Result<()> {
        panic!("uventet statuspublisering")
    }
}

// ---------------------------------------------------------------------------
// Listener
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Fakes {
    read: Arc<FakeReadRepository>,
    retry: Arc<FakeRetryRepository>,
    forkast: Arc<FakeForkastRepository>,
}

impl Fakes {
    fn listener(&self, transport: Arc<FakeTransport>) -> AdminListener {
        let validator = ValidateCommandService::new(
            Box::new(UventetKall),
            Box::new(UventetKall),
            Box::new(UventetKall),
            Box::new(UventetKall),
        );
        let services = AdminServices {
            read: Arc::new(AdminReadService::new(self.read.clone())),
            retry: Arc::new(AdminRetryService::new(
                self.retry.clone(),
                Arc::new(UventetKall),
                Arc::new(UventetKall),
                Arc::new(UventetKall),
                validator,
            )),
            forkast: Arc::new(AdminForkastService::new(self.forkast.clone())),
        };
        AdminListener::new(transport, services, Helse::new(), CancellationToken::new())
    }
}

fn melding(reply: &str, payload: Value) -> AdminMessage {
    AdminMessage {
        reply: Some(reply.to_string()),
        headers: None,
        payload: serde_json::to_vec(&payload).unwrap().into(),
    }
}

/// Kjører listeneren til alle ruter har subscribet og `antall` svar er
/// publisert, og stenger den så ned.
async fn kjor_til_svar(listener: &AdminListener, transport: &FakeTransport, antall: usize) {
    timeout(Duration::from_secs(1), async {
        let (resultat, ()) = tokio::join!(listener.run_once(), async {
            loop {
                if transport.publisert.lock().unwrap().len() == antall
                    && transport.subscriptions.lock().unwrap().len() == AdminRute::ALLE.len()
                {
                    listener.shutdown.cancel();
                    return;
                }
                transport.endret.notified().await;
            }
        });
        resultat.expect("shutdown er normal slutt");
    })
    .await
    .expect("alle subscriptions skal avsluttes");
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

#[derive(Default, Clone)]
struct LoggFangst {
    linjer: Arc<Mutex<Vec<HashMap<String, String>>>>,
}

impl LoggFangst {
    fn installer(&self) -> tracing::subscriber::DefaultGuard {
        tracing::subscriber::set_default(tracing_subscriber::registry().with(LoggLag {
            fangst: self.clone(),
        }))
    }

    fn med_melding(&self, melding: &str) -> Vec<HashMap<String, String>> {
        self.linjer
            .lock()
            .unwrap()
            .iter()
            .filter(|linje| linje.get("message").is_some_and(|m| m == melding))
            .cloned()
            .collect()
    }
}

struct LoggLag {
    fangst: LoggFangst,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LoggLag {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut felter = HashMap::new();
        event.record(&mut FeltSamler(&mut felter));
        self.fangst.linjer.lock().unwrap().push(felter);
    }
}

struct FeltSamler<'a>(&'a mut HashMap<String, String>);

impl tracing::field::Visit for FeltSamler<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().to_string(), format!("{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
}

// ---------------------------------------------------------------------------
// Rutetabell og subscriptions
// ---------------------------------------------------------------------------

#[tokio::test]
async fn hvert_subject_ruter_til_sin_handling_med_stabil_queue_group() {
    let transport = Arc::new(FakeTransport::default());
    let fakes = Fakes::default();
    let id = Uuid::new_v4();
    let key = json!({"type": "skuffenId", "value": id});
    transport.meldinger.lock().unwrap().extend([
        (
            "arkiv.admin.read.command.hent",
            vec![melding(
                "reply.read.command",
                json!({"utfort_av": "op", "command_id": id}),
            )],
        ),
        (
            "arkiv.admin.read.sak.hent",
            vec![melding(
                "reply.read.sak",
                json!({"utfort_av": "op", "key": key}),
            )],
        ),
        (
            "arkiv.admin.write.retry.commands",
            vec![melding(
                "reply.retry.commands",
                json!({"utfort_av": "op", "command_ids": [id]}),
            )],
        ),
        (
            "arkiv.admin.write.retry.sak",
            vec![melding(
                "reply.retry.sak",
                json!({"utfort_av": "op", "key": key}),
            )],
        ),
        (
            "arkiv.admin.write.forkast.commands",
            vec![melding(
                "reply.forkast.commands",
                json!({"utfort_av": "op", "command_ids": [id]}),
            )],
        ),
    ]);
    let listener = fakes.listener(transport.clone());

    kjor_til_svar(&listener, &transport, 5).await;

    let mut read_kall = fakes.read.kall.lock().unwrap().clone();
    read_kall.sort();
    assert_eq!(read_kall, vec!["command", "sak"]);
    assert_eq!(
        *fakes.retry.sak_kall.lock().unwrap(),
        vec![AdminSakNokkel::SkuffenId(SkuffenSakId(id))]
    );
    assert_eq!(*fakes.retry.command_kall.lock().unwrap(), vec![vec![id]]);
    assert_eq!(*fakes.forkast.kall.lock().unwrap(), vec![vec![id]]);

    let mut subscriptions = transport.subscriptions.lock().unwrap().clone();
    subscriptions.sort();
    assert_eq!(
        subscriptions,
        vec![
            (
                "arkiv.admin.read.command.hent",
                "skuffen-admin-read-command-hent-v1"
            ),
            (
                "arkiv.admin.read.sak.hent",
                "skuffen-admin-read-sak-hent-v1"
            ),
            (
                "arkiv.admin.write.forkast.commands",
                "skuffen-admin-write-forkast-commands-v1"
            ),
            (
                "arkiv.admin.write.retry.commands",
                "skuffen-admin-write-retry-commands-v1"
            ),
            (
                "arkiv.admin.write.retry.sak",
                "skuffen-admin-write-retry-sak-v1"
            ),
        ]
    );

    let svar: HashMap<_, _> = transport
        .publisert
        .lock()
        .unwrap()
        .iter()
        .cloned()
        .collect();
    let feil = |melding: &str| json!({"status": "Error", "payload": {"message": melding}});
    let ok = json!({"status": "Ok", "payload": null});
    assert_eq!(svar["reply.read.command"], feil(COMMAND_NOT_FOUND));
    assert_eq!(svar["reply.read.sak"], feil(SAK_NOT_FOUND));
    assert_eq!(svar["reply.retry.commands"], feil(COMMAND_NOT_FOUND));
    assert_eq!(svar["reply.retry.sak"], ok);
    assert_eq!(svar["reply.forkast.commands"], ok);
}

#[tokio::test]
async fn avsluttet_subscription_paa_hvilken_som_helst_rute_avslutter_run_once() {
    for rute in AdminRute::ALLE {
        let transport = Arc::new(FakeTransport {
            avsluttet: Some(rute.subject()),
            ..Default::default()
        });
        let listener = Fakes::default().listener(transport);

        let feil = timeout(Duration::from_secs(1), listener.run_once())
            .await
            .expect("lukket strøm skal ikke vente på de andre strømmene")
            .unwrap_err();

        assert!(
            feil.to_string().contains(rute.subject()),
            "{rute:?}: {feil}"
        );
    }
}

#[tokio::test]
async fn shutdown_avslutter_alle_ventende_subscriptions_uten_feil() {
    let transport = Arc::new(FakeTransport::default());
    let listener = Fakes::default().listener(transport.clone());

    kjor_til_svar(&listener, &transport, 0).await;

    assert_eq!(
        transport.subscriptions.lock().unwrap().len(),
        AdminRute::ALLE.len()
    );
}

#[tokio::test]
async fn skrivehandlinger_logger_attribusjon_ved_start_og_ved_svar() {
    let fangst = LoggFangst::default();
    let _guard = fangst.installer();
    let transport = Arc::new(FakeTransport::default());
    let listener = Fakes::default().listener(transport);
    let id = Uuid::new_v4();
    let request = json!({"utfort_av": " op ", "command_ids": [id]});

    listener
        .handle_message(
            AdminRute::ForkastCommands,
            melding("reply", request.clone()),
        )
        .await;
    listener
        .handle_message(AdminRute::RetryCommands, melding("reply", request))
        .await;

    for (startet, utfort, admin_action, resultat) in [
        (
            "admin forkast startet",
            "admin forkast utført",
            "write.forkast.commands",
            "ok",
        ),
        (
            "admin retry startet",
            "admin retry utført",
            "write.retry.commands",
            "not_found",
        ),
    ] {
        let startet = fangst.med_melding(startet);
        assert_eq!(startet.len(), 1, "{admin_action}");
        assert_eq!(startet[0]["utfort_av"], "op");
        assert_eq!(startet[0]["admin_action"], admin_action);
        assert_eq!(startet[0]["command_ids"], format!("{:?}", vec![id]));

        let utfort = fangst.med_melding(utfort);
        assert_eq!(utfort.len(), 1, "{admin_action}");
        assert_eq!(utfort[0]["utfort_av"], "op");
        assert_eq!(utfort[0]["admin_action"], admin_action);
        assert_eq!(utfort[0]["resultat"], resultat);
    }
}
