use anyhow::Result;
use async_trait::async_trait;
use std::sync::{Arc, Mutex, OnceLock};
use uuid::Uuid;

use application::command::ports::id_mapping_port::IdMappingRepository;
use infrastructure::query::mapping::lookup::key_mapping_queries;
use lib_schemas::skuffen::command::commands::Command;

#[derive(Default, Clone)]
struct InMemoryIdMappingState {
    command_ids: Vec<Uuid>,
    client_to_skuffen: Vec<(Uuid, Uuid)>,
    skuffen_to_arkiv: Vec<(Uuid, String)>,
    arkiv_to_skuffen: Vec<(String, Uuid)>,
}

impl InMemoryIdMappingState {
    fn register_command(&mut self, command_id: Uuid) {
        if !self.command_ids.contains(&command_id) {
            self.command_ids.push(command_id);
        }
    }

    fn register_client_mapping(&mut self, client_reference: Uuid, skuffen_id: Uuid) {
        if !self
            .client_to_skuffen
            .iter()
            .any(|(client, _)| client == &client_reference)
        {
            self.client_to_skuffen.push((client_reference, skuffen_id));
        }
    }

    fn register_arkiv_mapping(&mut self, skuffen_id: Uuid, arkiv_id: String) {
        if !self
            .skuffen_to_arkiv
            .iter()
            .any(|(skuffen, _)| skuffen == &skuffen_id)
        {
            self.skuffen_to_arkiv.push((skuffen_id, arkiv_id.clone()));
        }
        if !self
            .arkiv_to_skuffen
            .iter()
            .any(|(arkiv, _)| arkiv == &arkiv_id)
        {
            self.arkiv_to_skuffen.push((arkiv_id, skuffen_id));
        }
    }
}

#[derive(Default, Clone)]
pub struct InMemoryIdMappingRepository {
    state: Arc<Mutex<InMemoryIdMappingState>>,
}

impl InMemoryIdMappingRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_arkiv_mapping(&self, skuffen_id: Uuid, arkiv_id: String) {
        self.state
            .lock()
            .unwrap()
            .register_arkiv_mapping(skuffen_id, arkiv_id);
    }
}

#[async_trait]
impl IdMappingRepository for InMemoryIdMappingRepository {
    async fn has_processed_command(&self, command_id: Uuid) -> Result<bool, anyhow::Error> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .command_ids
            .contains(&command_id))
    }

    async fn register_mapping(
        &self,
        command_id: Uuid,
        client_reference: Uuid,
        skuffen_id: Uuid,
        _command: &Command,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let mut state = self.state.lock().unwrap();
        state.register_command(command_id);
        state.register_client_mapping(client_reference, skuffen_id);
        if let Some(arkiv_id) = arkiv_id {
            state.register_arkiv_mapping(skuffen_id, arkiv_id);
        }
        Ok(())
    }

    async fn register_document_mapping(
        &self,
        command_id: Uuid,
        client_reference: Uuid,
        skuffen_id: Uuid,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let mut state = self.state.lock().unwrap();
        state.register_command(command_id);
        state.register_client_mapping(client_reference, skuffen_id);
        if let Some(arkiv_id) = arkiv_id {
            state.register_arkiv_mapping(skuffen_id, arkiv_id);
        }
        Ok(())
    }

    async fn oppdater_arkiv_id_for_client_reference(
        &self,
        client_reference: Uuid,
        arkiv_id: String,
    ) -> Result<(), anyhow::Error> {
        let skuffen_id = {
            let state = self.state.lock().unwrap();
            state
                .client_to_skuffen
                .iter()
                .find(|(client, _)| client == &client_reference)
                .map(|(_, skuffen_id)| *skuffen_id)
        };
        if let Some(skuffen_id) = skuffen_id {
            let mut state = self.state.lock().unwrap();
            state.register_arkiv_mapping(skuffen_id, arkiv_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Fant ikke id_mapping for client_reference {}",
                client_reference
            ))
        }
    }

    async fn get_arkiv_id(&self, skuffen_id: Uuid) -> Result<Option<String>, anyhow::Error> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .skuffen_to_arkiv
            .iter()
            .find(|(skuffen, _)| skuffen == &skuffen_id)
            .map(|(_, arkiv)| arkiv.clone()))
    }

    async fn get_skuffen_id(&self, client_reference: Uuid) -> Result<Option<Uuid>, anyhow::Error> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .client_to_skuffen
            .iter()
            .find(|(client, _)| client == &client_reference)
            .map(|(_, skuffen)| *skuffen))
    }

    async fn get_skuffen_id_from_arkiv_id(
        &self,
        arkiv_id: &str,
    ) -> Result<Option<Uuid>, anyhow::Error> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .arkiv_to_skuffen
            .iter()
            .find(|(arkiv, _)| arkiv == arkiv_id)
            .map(|(_, skuffen)| *skuffen))
    }

    async fn ensure_arkiv_mapping(
        &self,
        _entity_type: &str,
        arkiv_id: &str,
    ) -> Result<Uuid, anyhow::Error> {
        let existing = {
            let state = self.state.lock().unwrap();
            state
                .arkiv_to_skuffen
                .iter()
                .find(|(existing_arkiv_id, _)| existing_arkiv_id == arkiv_id)
                .map(|(_, existing_skuffen_id)| *existing_skuffen_id)
        };
        if let Some(existing_skuffen_id) = existing {
            return Ok(existing_skuffen_id);
        }

        let skuffen_id = Uuid::now_v7();
        let mut state = self.state.lock().unwrap();
        state.register_arkiv_mapping(skuffen_id, arkiv_id.to_string());
        Ok(skuffen_id)
    }

    async fn delete_arkiv_mapping(
        &self,
        _entity_type: &str,
        arkiv_id: &str,
    ) -> Result<(), anyhow::Error> {
        let mut state = self.state.lock().unwrap();
        state.arkiv_to_skuffen.retain(|(arkiv, _)| arkiv != arkiv_id);
        state.skuffen_to_arkiv.retain(|(_, arkiv)| arkiv != arkiv_id);
        Ok(())
    }
}

static SHARED_ID_MAPPING: OnceLock<InMemoryIdMappingRepository> = OnceLock::new();

pub fn shared_id_mapping() -> InMemoryIdMappingRepository {
    SHARED_ID_MAPPING
        .get_or_init(InMemoryIdMappingRepository::new)
        .clone()
}

pub fn init_query_id_mapping(repo: Arc<dyn IdMappingRepository + Send + Sync>) {
    key_mapping_queries::init_id_mapping_repo(repo);
}
