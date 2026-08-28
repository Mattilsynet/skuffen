use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::operasjon::EntitetId;
use uuid::Uuid;

use crate::admin::model::{AdminCommand, AdminEntitetIdentitet, AdminSak, AdminSakNokkel};
use crate::admin::ports::admin_read_repository::AdminReadRepository;
use crate::admin::services::admin_read_service::{AdminReadError, AdminReadService};

#[derive(Clone)]
enum Svar<T> {
    Funnet(T),
    IkkeFunnet,
    Feil(String),
}

#[derive(Clone)]
struct FakeAdminReadRepository {
    command: Arc<Mutex<Svar<AdminCommand>>>,
    sak: Arc<Mutex<Svar<AdminSak>>>,
}

impl FakeAdminReadRepository {
    fn new(command: Svar<AdminCommand>, sak: Svar<AdminSak>) -> Self {
        Self {
            command: Arc::new(Mutex::new(command)),
            sak: Arc::new(Mutex::new(sak)),
        }
    }
}

#[async_trait]
impl AdminReadRepository for FakeAdminReadRepository {
    async fn hent_command(&self, _command_id: Uuid) -> Result<Option<AdminCommand>, anyhow::Error> {
        match self.command.lock().unwrap().clone() {
            Svar::Funnet(command) => Ok(Some(command)),
            Svar::IkkeFunnet => Ok(None),
            Svar::Feil(melding) => Err(anyhow::anyhow!(melding)),
        }
    }

    async fn hent_sak(&self, _key: AdminSakNokkel) -> Result<Option<AdminSak>, anyhow::Error> {
        match self.sak.lock().unwrap().clone() {
            Svar::Funnet(sak) => Ok(Some(sak)),
            Svar::IkkeFunnet => Ok(None),
            Svar::Feil(melding) => Err(anyhow::anyhow!(melding)),
        }
    }
}

fn tidspunkt() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-08-27T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn command(command_id: Uuid) -> AdminCommand {
    AdminCommand {
        command_id,
        correlation_id: None,
        command_type: "opprett_sak".to_string(),
        mottatt_at: tidspunkt(),
        dispatchet_at: None,
        dekomponert_at: None,
        operasjoner: Vec::new(),
    }
}

fn identity_only_sak(sak_id: SkuffenSakId) -> AdminSak {
    AdminSak {
        identitet: AdminEntitetIdentitet {
            skuffen_id: EntitetId::Sak(sak_id),
            client_reference: Some(Uuid::new_v4()),
            arkiv_id: None,
            created_at: tidspunkt(),
            updated_at: tidspunkt(),
        },
        fakta: None,
        operasjoner: Vec::new(),
    }
}

fn service(repository: FakeAdminReadRepository) -> AdminReadService {
    AdminReadService::new(Arc::new(repository))
}

#[tokio::test]
async fn hent_command_returnerer_lagret_command() {
    let command_id = Uuid::new_v4();
    let service = service(FakeAdminReadRepository::new(
        Svar::Funnet(command(command_id)),
        Svar::IkkeFunnet,
    ));

    let hentet = service.hent_command(command_id).await.unwrap();

    assert_eq!(hentet.command_id, command_id);
}

#[tokio::test]
async fn ukjent_command_er_typet_not_found() {
    let service = service(FakeAdminReadRepository::new(
        Svar::IkkeFunnet,
        Svar::IkkeFunnet,
    ));

    let feil = service.hent_command(Uuid::new_v4()).await.unwrap_err();

    assert!(matches!(feil, AdminReadError::CommandNotFound));
}

#[tokio::test]
async fn repositoryfeil_paa_command_blir_ikke_not_found() {
    let service = service(FakeAdminReadRepository::new(
        Svar::Feil("db nede".to_string()),
        Svar::IkkeFunnet,
    ));

    let feil = service.hent_command(Uuid::new_v4()).await.unwrap_err();

    assert!(matches!(feil, AdminReadError::Repository(_)));
}

#[tokio::test]
async fn identity_only_sak_er_success_ikke_not_found() {
    let sak_id = SkuffenSakId(Uuid::new_v4());
    let service = service(FakeAdminReadRepository::new(
        Svar::IkkeFunnet,
        Svar::Funnet(identity_only_sak(sak_id)),
    ));

    let sak = service
        .hent_sak(AdminSakNokkel::SkuffenId(sak_id))
        .await
        .unwrap();

    assert_eq!(sak.identitet.skuffen_id, EntitetId::Sak(sak_id));
    assert!(sak.fakta.is_none());
}

#[tokio::test]
async fn ukjent_sak_er_typet_not_found() {
    let service = service(FakeAdminReadRepository::new(
        Svar::IkkeFunnet,
        Svar::IkkeFunnet,
    ));

    let feil = service
        .hent_sak(AdminSakNokkel::ArkivId("2026/12345".to_string()))
        .await
        .unwrap_err();

    assert!(matches!(feil, AdminReadError::SakNotFound));
}

#[tokio::test]
async fn repositoryfeil_paa_sak_blir_ikke_not_found() {
    let service = service(FakeAdminReadRepository::new(
        Svar::IkkeFunnet,
        Svar::Feil("db nede".to_string()),
    ));

    let feil = service
        .hent_sak(AdminSakNokkel::ClientReference(Uuid::new_v4()))
        .await
        .unwrap_err();

    assert!(matches!(feil, AdminReadError::Repository(_)));
}
