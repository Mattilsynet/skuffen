use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use uuid::Uuid;

use crate::admin::ports::admin_forkast_port::{AdminForkastRepository, ForkastFeil};
use crate::admin::services::admin_forkast_service::{AdminForkastError, AdminForkastService};

#[derive(Default)]
struct FakeForkastRepository {
    ukjente: Vec<Uuid>,
    forkastet: Mutex<Vec<Uuid>>,
}

#[async_trait]
impl AdminForkastRepository for FakeForkastRepository {
    async fn forkast(&self, command_ids: &[Uuid]) -> Result<(), ForkastFeil> {
        if !self.ukjente.is_empty() {
            return Err(ForkastFeil::CommandNotFound(self.ukjente.clone()));
        }
        self.forkastet
            .lock()
            .unwrap()
            .extend_from_slice(command_ids);
        Ok(())
    }
}

fn oppsett(repository: FakeForkastRepository) -> (Arc<FakeForkastRepository>, AdminForkastService) {
    let repository = Arc::new(repository);
    let service = AdminForkastService::new(repository.clone());
    (repository, service)
}

#[tokio::test]
async fn duplikater_forkastes_en_gang() {
    let (repository, service) = oppsett(FakeForkastRepository::default());
    let command_id = Uuid::new_v4();

    service.handle(vec![command_id, command_id]).await.unwrap();

    assert_eq!(*repository.forkastet.lock().unwrap(), vec![command_id]);
}

#[tokio::test]
async fn ukjent_kommando_avviser_foresporselen() {
    let ukjent = Uuid::new_v4();
    let (_, service) = oppsett(FakeForkastRepository {
        ukjente: vec![ukjent],
        ..Default::default()
    });

    let feil = service.handle(vec![ukjent]).await.unwrap_err();

    assert!(matches!(feil, AdminForkastError::CommandNotFound(ids) if ids == vec![ukjent]));
}

#[tokio::test]
async fn tom_eller_for_stor_foresporsel_avvises_foer_repository() {
    let (repository, service) = oppsett(FakeForkastRepository::default());

    assert!(matches!(
        service.handle(Vec::new()).await,
        Err(AdminForkastError::UgyldigAntall)
    ));
    let for_mange = (0..101).map(|_| Uuid::new_v4()).collect();
    assert!(matches!(
        service.handle(for_mange).await,
        Err(AdminForkastError::UgyldigAntall)
    ));
    assert!(repository.forkastet.lock().unwrap().is_empty());
}
