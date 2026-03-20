use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use lib_schemas::skuffen::command::journalpost::{
    JournalpostCommon, OpprettInterntNotatJournalpost,
};
use lib_schemas::skuffen::dokument::Dokument;
use lib_schemas::skuffen::query::queries::SakKey;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::command::ports::eksekvering_state_port::{
    DokumentState, EksekveringKommando, EksekveringStateRepository, EksekveringStatus,
    JournalpostState, SakState, SakStatus,
};
use crate::command::ports::id_mapping_port::IdMappingRepository;
use crate::command::ports::registrer_eksekvering_port::RegistrerEksekveringUseCase;
use crate::command::services::registrer_eksekvering::RegistrerEksekveringService;

#[derive(Clone, Default)]
struct FakeEksekveringStateRepository {
    data: Arc<Mutex<FakeStateData>>,
}

#[derive(Default)]
struct FakeStateData {
    sak_state: Option<(Uuid, SakState)>,
    journalpost_state: Option<(Uuid, Uuid, JournalpostState)>,
    registrerte_kommandoer: Vec<Uuid>,
    existing_sak: Option<SakState>,
    existing_journalpost: Option<JournalpostState>,
}

#[async_trait]
impl EksekveringStateRepository for FakeEksekveringStateRepository {
    async fn hent_sak_state(&self, _sak_id: Uuid) -> Result<Option<SakState>, anyhow::Error> {
        Ok(self.data.lock().unwrap().existing_sak.clone())
    }

    async fn lagre_sak_state(&self, sak_id: Uuid, state: SakState) -> Result<(), anyhow::Error> {
        self.data.lock().unwrap().sak_state = Some((sak_id, state));
        Ok(())
    }

    async fn hent_journalpost_state(
        &self,
        _journalpost_id: Uuid,
    ) -> Result<Option<JournalpostState>, anyhow::Error> {
        Ok(self.data.lock().unwrap().existing_journalpost.clone())
    }

    async fn lagre_journalpost_state(
        &self,
        journalpost_id: Uuid,
        sak_id: Uuid,
        state: JournalpostState,
    ) -> Result<(), anyhow::Error> {
        self.data.lock().unwrap().journalpost_state = Some((journalpost_id, sak_id, state));
        Ok(())
    }

    async fn hent_journalposter_for_sak(
        &self,
        _sak_id: Uuid,
    ) -> Result<Vec<JournalpostState>, anyhow::Error> {
        Ok(Vec::new())
    }

    async fn hent_dokument_state(
        &self,
        _dokument_id: Uuid,
    ) -> Result<Option<DokumentState>, anyhow::Error> {
        Ok(None)
    }

    async fn lagre_dokument_state(
        &self,
        _dokument_id: Uuid,
        _journalpost_id: Uuid,
        _state: DokumentState,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn oppdater_eksekvering(
        &self,
        _command_id: Uuid,
        _status: EksekveringStatus,
        _last_error: Option<String>,
        _next_retry_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn registrer_kommando(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<(), anyhow::Error> {
        self.data
            .lock()
            .unwrap()
            .registrerte_kommandoer
            .push(envelope.command_id);
        Ok(())
    }

    async fn hent_klare_kommandoer(
        &self,
        _limit: i64,
        _worker_id: &str,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error> {
        Ok(Vec::new())
    }
}

#[derive(Clone, Default)]
struct FakeIdMappingRepository {
    data: Arc<Mutex<FakeIdMappingData>>,
}

#[derive(Default)]
struct FakeIdMappingData {
    ensure_calls: Vec<(String, String)>,
    ensured_sak_id: Option<Uuid>,
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
        _skuffen_id: Uuid,
        _command: &Command,
        _arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn register_document_mapping(
        &self,
        _command_id: Uuid,
        _client_reference: Uuid,
        _skuffen_id: Uuid,
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

    async fn get_arkiv_id(&self, _skuffen_id: Uuid) -> Result<Option<String>, anyhow::Error> {
        Ok(None)
    }

    async fn get_skuffen_id(&self, _client_reference: Uuid) -> Result<Option<Uuid>, anyhow::Error> {
        Ok(None)
    }

    async fn get_skuffen_id_from_arkiv_id(
        &self,
        _arkiv_id: &str,
    ) -> Result<Option<Uuid>, anyhow::Error> {
        Ok(None)
    }

    async fn ensure_arkiv_mapping(
        &self,
        entity_type: &str,
        arkiv_id: &str,
    ) -> Result<Uuid, anyhow::Error> {
        let mut data = self.data.lock().unwrap();
        data.ensure_calls
            .push((entity_type.to_string(), arkiv_id.to_string()));
        Ok(data.ensured_sak_id.unwrap_or_else(Uuid::new_v4))
    }

    async fn delete_arkiv_mapping(
        &self,
        _entity_type: &str,
        _arkiv_id: &str,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn registrer_eksekvering_seeds_state_for_client_reference_sak() {
    let state_repo = FakeEksekveringStateRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let service = RegistrerEksekveringService::new(
        Box::new(state_repo.clone()),
        Box::new(id_mapping_repo.clone()),
    );

    let sak_id = Uuid::new_v4();
    let journalpost_id = Uuid::new_v4();
    let envelope = make_journalpost_command(journalpost_id, SakKey::ClientReference(sak_id));

    service.handle(&envelope).await.unwrap();

    let state = state_repo.data.lock().unwrap();
    let (saved_sak_id, saved_sak_state) = state.sak_state.clone().unwrap();
    assert_eq!(saved_sak_id, sak_id);
    assert_eq!(saved_sak_state.status, SakStatus::UnderBehandling);
    assert!(!saved_sak_state.opprettet);
    assert_eq!(saved_sak_state.saksnummer, None);

    let (saved_journalpost_id, linked_sak_id, saved_journalpost_state) =
        state.journalpost_state.clone().unwrap();
    assert_eq!(saved_journalpost_id, journalpost_id);
    assert_eq!(linked_sak_id, sak_id);
    assert_eq!(saved_journalpost_state.journalposttype, 'X');
    assert_eq!(state.registrerte_kommandoer, vec![envelope.command_id]);

    let id_mapping = id_mapping_repo.data.lock().unwrap();
    assert!(id_mapping.ensure_calls.is_empty());
}

#[tokio::test]
async fn registrer_eksekvering_ensures_mapping_for_arkiv_id_sak() {
    let state_repo = FakeEksekveringStateRepository::default();
    let id_mapping_repo = FakeIdMappingRepository::default();
    let ensured_sak_id = Uuid::new_v4();
    id_mapping_repo.data.lock().unwrap().ensured_sak_id = Some(ensured_sak_id);

    let service = RegistrerEksekveringService::new(
        Box::new(state_repo.clone()),
        Box::new(id_mapping_repo.clone()),
    );

    let journalpost_id = Uuid::new_v4();
    let envelope = make_journalpost_command(
        journalpost_id,
        SakKey::ArkivId(lib_schemas::skuffen::sak::Saksnummer::new("2025/123").unwrap()),
    );

    service.handle(&envelope).await.unwrap();

    let state = state_repo.data.lock().unwrap();
    let (saved_sak_id, saved_sak_state) = state.sak_state.clone().unwrap();
    assert_eq!(saved_sak_id, ensured_sak_id);
    assert!(saved_sak_state.opprettet);
    assert_eq!(saved_sak_state.saksnummer.as_deref(), Some("2025/123"));

    let (_, linked_sak_id, _) = state.journalpost_state.clone().unwrap();
    assert_eq!(linked_sak_id, ensured_sak_id);

    let id_mapping = id_mapping_repo.data.lock().unwrap();
    assert_eq!(
        id_mapping.ensure_calls,
        vec![("sak".to_string(), "2025/123".to_string())]
    );
}

fn make_journalpost_command(journalpost_id: Uuid, sak_key: SakKey) -> CommandEnvelope<Command> {
    CommandEnvelope {
        command_id: Uuid::new_v4(),
        correlation_id: Some(Uuid::new_v4()),
        payload: Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
            felles: JournalpostCommon {
                client_reference: journalpost_id,
                tittel: "Internt notat".to_string(),
                dokument_dato: "2025-01-01".to_string(),
                saksbehandler: "Z12345".to_string(),
                saksbehandler_enhet: "42".to_string(),
                tilgang: None,
                dokumenter: vec![Dokument {
                    client_reference: Uuid::new_v4(),
                    tittel: "Vedlegg".to_string(),
                    filtype: "PDF".to_string(),
                    dokument_referanse: Uuid::new_v4(),
                }],
                sak_key,
                kildesystem: None,
            },
        }),
    }
}
