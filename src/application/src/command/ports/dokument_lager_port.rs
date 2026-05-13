use async_trait::async_trait;
use uuid::Uuid;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DokumentMetadata {
    pub origin: Option<String>,
    pub source_template_reference: Option<Uuid>,
    pub source_document_id: Option<Uuid>,
    pub source_command_id: Option<Uuid>,
    pub render_timestamp: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DokumentFil {
    pub id: Uuid,
    pub data: Vec<u8>,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub metadata: DokumentMetadata,
}

#[async_trait]
pub trait DokumentLager: Send + Sync {
    async fn save(&self, file: DokumentFil) -> Result<(), anyhow::Error>;
    async fn get(&self, id: Uuid) -> Result<Option<DokumentFil>, anyhow::Error>;
}

#[cfg(test)]
pub mod fake {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use uuid::Uuid;

    use super::{DokumentFil, DokumentLager};

    #[derive(Clone, Default)]
    pub struct FakeDokumentLager {
        files: Arc<Mutex<HashMap<Uuid, DokumentFil>>>,
    }

    impl FakeDokumentLager {
        pub fn with_files(files: Vec<DokumentFil>) -> Self {
            Self {
                files: Arc::new(Mutex::new(
                    files.into_iter().map(|file| (file.id, file)).collect(),
                )),
            }
        }

        pub fn saved(&self, id: Uuid) -> Option<DokumentFil> {
            self.files.lock().unwrap().get(&id).cloned()
        }
    }

    #[async_trait]
    impl DokumentLager for FakeDokumentLager {
        async fn save(&self, file: DokumentFil) -> Result<(), anyhow::Error> {
            self.files.lock().unwrap().insert(file.id, file);
            Ok(())
        }

        async fn get(&self, id: Uuid) -> Result<Option<DokumentFil>, anyhow::Error> {
            Ok(self.files.lock().unwrap().get(&id).cloned())
        }
    }
}
