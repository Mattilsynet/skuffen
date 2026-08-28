use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use domain::eksekvering::operasjon::EntitetType;
use domain::eksekvering::typer::{CommandEvent, CommandStatus, Operasjonstatus};
use uuid::Uuid;

use crate::command::ports::command_dispatcher_port::CommandDispatcher;
use crate::command::ports::command_port::{CommandRepository, Mottaksresultat};
use crate::command::ports::entitet_port::{Entitet, EntitetRepository, NyEntitet};
use crate::command::ports::status_publisher_port::StatusPublisher;
use crate::command::services::ingest_command::IngestCommandService;
use crate::command::{Command, CommandEnvelope, test_fixtures};

// ---------------------------------------------------------------------------
// Fakes
// ---------------------------------------------------------------------------

/// Mottaksjournalen. Idempotency-nøkkelen er dispatch-milepælen, ikke radens
/// eksistens (SKU-0016 R11), så fake-en må skille de to eksplisitt.
#[derive(Clone, Default)]
struct FakeCommandRepository {
    mottatt: Arc<Mutex<HashMap<Uuid, bool>>>,
}

#[async_trait]
impl CommandRepository for FakeCommandRepository {
    async fn registrer_mottatt(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<Mottaksresultat, anyhow::Error> {
        let mut mottatt = self.mottatt.lock().unwrap();
        match mottatt.get(&envelope.command_id) {
            Some(true) => Ok(Mottaksresultat::AlleredeDispatchet),
            Some(false) => Ok(Mottaksresultat::MottattIkkeDispatchet),
            None => {
                mottatt.insert(envelope.command_id, false);
                Ok(Mottaksresultat::Ny)
            }
        }
    }

    async fn marker_dispatchet(&self, command_id: Uuid) -> Result<(), anyhow::Error> {
        self.mottatt.lock().unwrap().insert(command_id, true);
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeEntitetRepository {
    /// client_reference -> (skuffen_id, type)
    entiteter: Arc<Mutex<HashMap<Uuid, (Uuid, EntitetType)>>>,
    arkiv: Arc<Mutex<HashMap<String, Uuid>>>,
}

#[async_trait]
impl EntitetRepository for FakeEntitetRepository {
    async fn registrer(&self, entitet: NyEntitet) -> Result<Uuid, anyhow::Error> {
        let Some(client_reference) = entitet.client_reference else {
            return Ok(entitet.skuffen_id);
        };
        let mut entiteter = self.entiteter.lock().unwrap();
        // Eksisterende rad vinner, slik at en replay gjenbruker id-ene.
        let effektiv = entiteter
            .entry(client_reference)
            .or_insert((entitet.skuffen_id, entitet.entitet_type));
        Ok(effektiv.0)
    }

    async fn hent_for_client_reference(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<Entitet>, anyhow::Error> {
        Ok(self.entiteter.lock().unwrap().get(&client_reference).map(
            |(skuffen_id, entitet_type)| Entitet {
                skuffen_id: *skuffen_id,
                entitet_type: *entitet_type,
                client_reference: Some(client_reference),
                arkiv_id: None,
            },
        ))
    }

    async fn hent_for_arkiv_id(
        &self,
        _entitet_type: EntitetType,
        _arkiv_id: &str,
    ) -> Result<Option<Entitet>, anyhow::Error> {
        Ok(None)
    }

    async fn hent_eller_opprett_for_arkiv_id(
        &self,
        _entitet_type: EntitetType,
        arkiv_id: &str,
    ) -> Result<Uuid, anyhow::Error> {
        let mut arkiv = self.arkiv.lock().unwrap();
        Ok(*arkiv
            .entry(arkiv_id.to_string())
            .or_insert_with(Uuid::now_v7))
    }

    async fn hent_arkiv_id(&self, _skuffen_id: Uuid) -> Result<Option<String>, anyhow::Error> {
        Ok(None)
    }
}

#[derive(Clone, Default)]
struct FakeCommandDispatcher {
    should_fail: bool,
    dispatched: Arc<Mutex<Vec<Uuid>>>,
}

#[async_trait]
impl CommandDispatcher for FakeCommandDispatcher {
    async fn dispatch(&self, envelope: &CommandEnvelope<Command>) -> Result<(), anyhow::Error> {
        if self.should_fail {
            return Err(anyhow::anyhow!("nats nede"));
        }
        self.dispatched.lock().unwrap().push(envelope.command_id);
        Ok(())
    }
}

#[derive(Clone, Default)]
struct FakeStatusPublisher {
    command_status: Arc<Mutex<Vec<CommandStatus>>>,
}

#[async_trait]
impl StatusPublisher for FakeStatusPublisher {
    async fn publiser_command_status(&self, status: CommandStatus) -> Result<(), anyhow::Error> {
        self.command_status.lock().unwrap().push(status);
        Ok(())
    }

    async fn publiser_operasjonstatus(
        &self,
        _status: Operasjonstatus,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

fn build_service(
    command: FakeCommandRepository,
    entitet: FakeEntitetRepository,
    dispatcher: FakeCommandDispatcher,
    publisher: FakeStatusPublisher,
) -> IngestCommandService {
    IngestCommandService::new(
        Box::new(command),
        Box::new(entitet),
        Box::new(dispatcher),
        Box::new(publisher),
    )
}

fn opprett_sak_batch(command_id: Uuid, client_reference: Uuid) -> Vec<CommandEnvelope<Command>> {
    vec![test_fixtures::opprett_sak_envelope(
        command_id,
        client_reference,
    )]
}

// ---------------------------------------------------------------------------
// Regresjonstest for ingest-defekten SKU-0016 R11 fikser.
// ---------------------------------------------------------------------------

/// v2 skrev idempotency-markøren (daværende `id_mapping`, som bar `command_id`) **før**
/// dispatch. Feilet dispatch, fikk klienten `Error` for batchen — men markøren
/// lå igjen. Ved klient-retry svarte `has_processed_command` `true`, kommandoen
/// ble hoppet over som «allerede behandlet», og klienten fikk `Ok` med
/// command_id i kvitteringen for noe som aldri ble dispatchet, aldri validert
/// og aldri eksekvert.
///
/// v3 flytter milepælen til etter dispatch: `dispatchet_at` er nøkkelen, ikke
/// radens eksistens.
#[tokio::test]
async fn retry_etter_dispatch_feil_skal_ikke_kvittere_ok_for_udispatchet_command() {
    let command = FakeCommandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    // Forsøk 1 — NATS nede, klienten får feil.
    let failing = FakeCommandDispatcher {
        should_fail: true,
        ..Default::default()
    };
    let service = build_service(
        command.clone(),
        entitet.clone(),
        failing,
        FakeStatusPublisher::default(),
    );
    assert!(
        service
            .handle(opprett_sak_batch(command_id, client_reference))
            .await
            .is_err()
    );

    // Forsøk 2 — NATS oppe igjen, klienten reprøver samme batch.
    let working = FakeCommandDispatcher::default();
    let service = build_service(
        command,
        entitet,
        working.clone(),
        FakeStatusPublisher::default(),
    );
    let command_ids = service
        .handle(opprett_sak_batch(command_id, client_reference))
        .await
        .expect("retry rapporteres som akseptert");

    // Kvitteringen lover at kommandoen er akseptert...
    assert_eq!(command_ids, vec![command_id]);

    // ...da må den også faktisk være dispatchet.
    let dispatched = working.dispatched.lock().unwrap();
    assert_eq!(
        dispatched.len(),
        1,
        "command kvittert som akseptert må være dispatchet, men ble dispatchet {} ganger",
        dispatched.len()
    );
}

#[tokio::test]
async fn ekte_duplikat_dispatches_ikke_paa_nytt() {
    let command = FakeCommandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let dispatcher = FakeCommandDispatcher::default();
    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    for _ in 0..2 {
        let service = build_service(
            command.clone(),
            entitet.clone(),
            dispatcher.clone(),
            FakeStatusPublisher::default(),
        );
        let command_ids = service
            .handle(opprett_sak_batch(command_id, client_reference))
            .await
            .expect("duplikat aksepteres idempotent");
        assert_eq!(command_ids, vec![command_id]);
    }

    assert_eq!(
        dispatcher.dispatched.lock().unwrap().len(),
        1,
        "en allerede dispatchet command skal ikke sendes igjen"
    );
}

#[tokio::test]
async fn replay_etter_dispatch_feil_gjenbruker_skuffen_id() {
    let command = FakeCommandRepository::default();
    let entitet = FakeEntitetRepository::default();
    let command_id = Uuid::new_v4();
    let client_reference = Uuid::new_v4();

    let failing = FakeCommandDispatcher {
        should_fail: true,
        ..Default::default()
    };
    let service = build_service(
        command.clone(),
        entitet.clone(),
        failing,
        FakeStatusPublisher::default(),
    );
    let _ = service
        .handle(opprett_sak_batch(command_id, client_reference))
        .await;

    let forste = entitet
        .hent_for_client_reference(client_reference)
        .await
        .unwrap()
        .expect("entitet mintet ved første forsøk");

    let service = build_service(
        command,
        entitet.clone(),
        FakeCommandDispatcher::default(),
        FakeStatusPublisher::default(),
    );
    service
        .handle(opprett_sak_batch(command_id, client_reference))
        .await
        .unwrap();

    let etter = entitet
        .hent_for_client_reference(client_reference)
        .await
        .unwrap()
        .expect("entitet finnes fortsatt");

    assert_eq!(
        forste.skuffen_id, etter.skuffen_id,
        "en replay skal gjenbruke id-ene fra første forsøk, ikke minte nye"
    );
}

#[tokio::test]
async fn mottatt_publiseres_forst_etter_vellykket_dispatch() {
    let publisher = FakeStatusPublisher::default();

    let failing = FakeCommandDispatcher {
        should_fail: true,
        ..Default::default()
    };
    let service = build_service(
        FakeCommandRepository::default(),
        FakeEntitetRepository::default(),
        failing,
        publisher.clone(),
    );
    let _ = service
        .handle(opprett_sak_batch(Uuid::new_v4(), Uuid::new_v4()))
        .await;

    assert!(
        publisher.command_status.lock().unwrap().is_empty(),
        "ingen status skal love mottak når dispatch feilet"
    );

    let service = build_service(
        FakeCommandRepository::default(),
        FakeEntitetRepository::default(),
        FakeCommandDispatcher::default(),
        publisher.clone(),
    );
    service
        .handle(opprett_sak_batch(Uuid::new_v4(), Uuid::new_v4()))
        .await
        .unwrap();

    let publisert = publisher.command_status.lock().unwrap();
    assert_eq!(publisert.len(), 1);
    assert_eq!(publisert[0].hendelse, CommandEvent::Mottatt);
    assert!(!publisert[0].terminal);
}

#[tokio::test]
async fn batch_beholder_rekkefolgen() {
    let dispatcher = FakeCommandDispatcher::default();
    let service = build_service(
        FakeCommandRepository::default(),
        FakeEntitetRepository::default(),
        dispatcher.clone(),
        FakeStatusPublisher::default(),
    );

    let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
    let envelopes: Vec<CommandEnvelope<Command>> = ids
        .iter()
        .map(|id| test_fixtures::opprett_sak_envelope(*id, Uuid::new_v4()))
        .collect();

    let command_ids = service.handle(envelopes).await.unwrap();

    assert_eq!(command_ids, ids);
    assert_eq!(*dispatcher.dispatched.lock().unwrap(), ids);
}
