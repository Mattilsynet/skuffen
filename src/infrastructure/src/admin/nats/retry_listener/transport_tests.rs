use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use application::admin::model::AdminSakNokkel;
use application::admin::ports::admin_retry_port::{
    AdminRetryRepository, FrigivelseFeil, OriginalCommandKilde, RetryGrunnlag,
};
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
use futures::stream;
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::time::timeout;

use super::*;
use crate::admin::nats::listener::AdminMessageStream;

#[derive(Default)]
struct FakeRepository {
    sak_kall: Mutex<Vec<AdminSakNokkel>>,
    command_kall: Mutex<Vec<Vec<Uuid>>>,
    teknisk_feil: bool,
}

#[async_trait]
impl AdminRetryRepository for FakeRepository {
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

#[derive(Default)]
struct FakeTransport {
    meldinger: Mutex<HashMap<&'static str, Vec<AdminMessage>>>,
    avsluttet: Option<&'static str>,
    subscriptions: Mutex<Vec<(&'static str, &'static str)>>,
    publisert: Mutex<Vec<(String, Value)>>,
    endret: Notify,
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
        self.publisert
            .lock()
            .unwrap()
            .push((reply, serde_json::from_slice(&payload).unwrap()));
        self.endret.notify_one();
        Ok(())
    }

    fn max_payload(&self) -> usize {
        usize::MAX
    }
}

fn bygg_listener(
    transport: Arc<FakeTransport>,
    repository: Arc<FakeRepository>,
) -> AdminRetryListener {
    let validator = ValidateCommandService::new(
        Box::new(UventetKall),
        Box::new(UventetKall),
        Box::new(UventetKall),
        Box::new(UventetKall),
    );
    let service = AdminRetryService::new(
        repository,
        Arc::new(UventetKall),
        Arc::new(UventetKall),
        Arc::new(UventetKall),
        validator,
    );
    AdminRetryListener::new(
        transport,
        Arc::new(service),
        Helse::new(),
        CancellationToken::new(),
    )
}

fn melding(reply: &str, payload: Value) -> AdminMessage {
    AdminMessage {
        reply: Some(reply.to_string()),
        headers: None,
        payload: serde_json::to_vec(&payload).unwrap().into(),
    }
}

async fn kjor_til_svar(listener: &AdminRetryListener, transport: &FakeTransport, antall: usize) {
    timeout(Duration::from_secs(1), async {
        let (resultat, ()) = tokio::join!(listener.run_once(), async {
            loop {
                if transport.publisert.lock().unwrap().len() == antall
                    && transport.subscriptions.lock().unwrap().len() == 2
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
    .expect("begge subscriptions skal avsluttes");
}

#[tokio::test]
async fn subjects_ruter_til_riktig_service_med_stabile_queue_groups() {
    let transport = Arc::new(FakeTransport::default());
    let repository = Arc::new(FakeRepository::default());
    let id = Uuid::new_v4();
    transport.meldinger.lock().unwrap().extend([
        (
            "arkiv.admin.write.retry.sak",
            vec![melding(
                "reply.sak",
                json!({
                    "utfort_av": " op ", "key": {"type": "skuffenId", "value": id}
                }),
            )],
        ),
        (
            "arkiv.admin.write.retry.commands",
            vec![melding(
                "reply.commands",
                json!({
                    "utfort_av": " op ", "command_ids": [id]
                }),
            )],
        ),
    ]);
    let listener = bygg_listener(transport.clone(), repository.clone());

    kjor_til_svar(&listener, &transport, 2).await;

    assert_eq!(
        *repository.sak_kall.lock().unwrap(),
        vec![AdminSakNokkel::SkuffenId(SkuffenSakId(id))]
    );
    assert_eq!(*repository.command_kall.lock().unwrap(), vec![vec![id]]);
    let mut subscriptions = transport.subscriptions.lock().unwrap().clone();
    subscriptions.sort();
    assert_eq!(
        subscriptions,
        vec![
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
    assert_eq!(svar["reply.sak"], json!({"status": "Ok", "payload": null}));
    assert_eq!(
        svar["reply.commands"],
        json!({"status": "Error", "payload": {"message": COMMAND_NOT_FOUND}})
    );
}

#[tokio::test]
async fn teknisk_feil_gir_sanitert_feilsvar_paa_begge_subjects() {
    let transport = Arc::new(FakeTransport::default());
    let repository = Arc::new(FakeRepository {
        teknisk_feil: true,
        ..Default::default()
    });
    let id = Uuid::new_v4();
    transport.meldinger.lock().unwrap().extend([
        (
            ADMIN_WRITE_RETRY_SAK_SUBJECT,
            vec![melding(
                "reply.sak",
                json!({"utfort_av": "op", "key": {"type": "skuffenId", "value": id}}),
            )],
        ),
        (
            ADMIN_WRITE_RETRY_COMMANDS_SUBJECT,
            vec![melding(
                "reply.commands",
                json!({"utfort_av": "op", "command_ids": [id]}),
            )],
        ),
    ]);
    let listener = bygg_listener(transport.clone(), repository);

    kjor_til_svar(&listener, &transport, 2).await;

    for (_, svar) in transport.publisert.lock().unwrap().iter() {
        assert_eq!(
            *svar,
            json!({"status": "Error", "payload": {"message": "Internal error"}})
        );
    }
}

#[tokio::test]
async fn feil_shape_paa_begge_subjects_avvises_uten_servicekall() {
    let transport = Arc::new(FakeTransport::default());
    let repository = Arc::new(FakeRepository::default());
    let id = Uuid::new_v4();
    let key = json!({"type": "skuffenId", "value": id});
    let ugyldige = [
        json!({"utfort_av": "op"}),
        json!({"utfort_av": "op", "key": key, "command_ids": [id]}),
        json!({"utfort_av": "op", "key": null}),
        json!({"utfort_av": "op", "command_ids": null}),
        json!({"utfort_av": "op", "key": key, "command_ids": null}),
        json!({"utfort_av": "op", "key": null, "command_ids": [id]}),
        json!({"utfort_av": "op", "key": key, "extra": true}),
        json!({"utfort_av": "op", "command_ids": [id], "extra": true}),
        json!({"utfort_av": "op", "key": {"type": "ukjent", "value": id}}),
        json!({"utfort_av": "op", "key": {"type": "skuffenId", "value": id, "extra": true}}),
        json!({"key": key}),
        json!({"command_ids": [id]}),
        json!(null),
    ];
    let mut antall = 0;
    for action in [RetryAction::Sak, RetryAction::Commands] {
        let feil_shape = match action {
            RetryAction::Sak => json!({"utfort_av": "op", "command_ids": [id]}),
            RetryAction::Commands => json!({"utfort_av": "op", "key": key}),
        };
        assert!(parse_request(action, &serde_json::to_vec(&feil_shape).unwrap()).is_err());
        let mut meldinger: Vec<_> = ugyldige
            .iter()
            .cloned()
            .chain([feil_shape])
            .map(|wire| melding("reply.invalid", wire))
            .collect();
        for utfort_av in [" ".to_string(), "a\nb".to_string(), "x".repeat(129)] {
            let wire = match action {
                RetryAction::Sak => json!({"utfort_av": utfort_av, "key": key}),
                RetryAction::Commands => json!({"utfort_av": utfort_av, "command_ids": [id]}),
            };
            meldinger.push(melding("reply.invalid", wire));
        }
        meldinger.push(AdminMessage {
            reply: Some("reply.invalid".to_string()),
            headers: None,
            payload: "{ugyldig json".into(),
        });
        antall += meldinger.len();
        transport
            .meldinger
            .lock()
            .unwrap()
            .insert(action.subject(), meldinger);
    }
    let listener = bygg_listener(transport.clone(), repository.clone());

    kjor_til_svar(&listener, &transport, antall).await;

    assert!(repository.sak_kall.lock().unwrap().is_empty());
    assert!(repository.command_kall.lock().unwrap().is_empty());
    for (reply, svar) in transport.publisert.lock().unwrap().iter() {
        assert_eq!(reply, "reply.invalid");
        assert_eq!(
            *svar,
            json!({"status": "Error", "payload": {"message": "Invalid request format"}})
        );
    }
}

async fn avsluttet_subscription_feiler(subject: &'static str) {
    let transport = Arc::new(FakeTransport {
        avsluttet: Some(subject),
        ..Default::default()
    });
    let listener = bygg_listener(transport, Arc::new(FakeRepository::default()));

    let feil = timeout(Duration::from_secs(1), listener.run_once())
        .await
        .expect("lukket strøm skal ikke vente på den andre strømmen")
        .unwrap_err();

    assert!(feil.to_string().contains(subject));
}

#[tokio::test]
async fn avsluttet_sak_subscription_avslutter_run_once() {
    avsluttet_subscription_feiler(ADMIN_WRITE_RETRY_SAK_SUBJECT).await;
}

#[tokio::test]
async fn avsluttet_commands_subscription_avslutter_run_once() {
    avsluttet_subscription_feiler(ADMIN_WRITE_RETRY_COMMANDS_SUBJECT).await;
}

#[tokio::test]
async fn shutdown_avslutter_begge_ventende_subscriptions_uten_feil() {
    let transport = Arc::new(FakeTransport::default());
    let listener = bygg_listener(transport.clone(), Arc::new(FakeRepository::default()));

    kjor_til_svar(&listener, &transport, 0).await;

    assert_eq!(transport.subscriptions.lock().unwrap().len(), 2);
}
