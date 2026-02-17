use application::command::ports::id_mapping_port::IdMappingRepository;
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::Command;
use sqlx::postgres::PgPool;
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresIdMappingRepository {
    pool: PgPool,
}

impl PostgresIdMappingRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl IdMappingRepository for PostgresIdMappingRepository {
    async fn has_processed_command(&self, command_id: Uuid) -> Result<bool, anyhow::Error> {
        let exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(SELECT 1 FROM id_mapping WHERE command_id = $1)
            "#,
        )
        .bind(command_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(exists)
    }

    async fn register_mapping(
        &self,
        command_id: Uuid,
        client_reference: Uuid,
        skuffen_id: Uuid,
        command: &Command,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        if let Some(existing_skuffen_id) = self.get_skuffen_id(client_reference).await? {
            if existing_skuffen_id != skuffen_id {
                return Err(anyhow::anyhow!(
                    "client_reference is already mapped to a different skuffen_id"
                ));
            }
            return Ok(());
        }

        let entity_type = match command {
            Command::OpprettSak(_) | Command::AvsluttSak(_) => "sak",
            Command::OpprettInngåendeJournalpost(_)
            | Command::OpprettUtgåendeJournalpost(_)
            | Command::OpprettInterntNotatJournalpost(_) => "journalpost",
        };

        // Insert into id_mapping.
        // We now have distinct client_reference and command_id.
        // On conflict (client_reference): do nothing (idempotent at entity level).
        // Command idempotency is checked prior to this call via has_processed_command,
        // but the constraint here protects data integrity.

        sqlx::query(
            r#"
            INSERT INTO id_mapping (skuffen_id, entity_type, client_reference, arkiv_id, command_id)
            VALUES ($1, $2::entity_type, $3, $4, $5)
            ON CONFLICT (client_reference) DO NOTHING
            "#,
        )
        .bind(skuffen_id)
        .bind(entity_type)
        .bind(client_reference)
        .bind(arkiv_id)
        .bind(command_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn register_document_mapping(
        &self,
        command_id: Uuid,
        client_reference: Uuid,
        skuffen_id: Uuid,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        if let Some(existing_skuffen_id) = self.get_skuffen_id(client_reference).await? {
            if existing_skuffen_id != skuffen_id {
                return Err(anyhow::anyhow!(
                    "client_reference is already mapped to a different skuffen_id"
                ));
            }
            return Ok(());
        }

        let entity_type = "dokument";

        sqlx::query(
            r#"
            INSERT INTO id_mapping (skuffen_id, entity_type, client_reference, arkiv_id, command_id)
            VALUES ($1, $2::entity_type, $3, $4, $5)
            ON CONFLICT (client_reference) DO NOTHING
            "#,
        )
        .bind(skuffen_id)
        .bind(entity_type)
        .bind(client_reference)
        .bind(arkiv_id)
        .bind(command_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_arkiv_id(&self, skuffen_id: Uuid) -> Result<Option<String>, anyhow::Error> {
        let arkiv_id: Option<String> = sqlx::query_scalar(
            r#"
            SELECT arkiv_id
            FROM id_mapping
            WHERE skuffen_id = $1
            "#,
        )
        .bind(skuffen_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(arkiv_id)
    }

    async fn get_skuffen_id(&self, client_reference: Uuid) -> Result<Option<Uuid>, anyhow::Error> {
        let skuffen_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT skuffen_id
            FROM id_mapping
            WHERE client_reference = $1
            "#,
        )
        .bind(client_reference)
        .fetch_optional(&self.pool)
        .await?;

        Ok(skuffen_id)
    }

    async fn get_skuffen_id_from_arkiv_id(
        &self,
        arkiv_id: &str,
    ) -> Result<Option<Uuid>, anyhow::Error> {
        let skuffen_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT skuffen_id
            FROM id_mapping
            WHERE arkiv_id = $1
            "#,
        )
        .bind(arkiv_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(skuffen_id)
    }
}
