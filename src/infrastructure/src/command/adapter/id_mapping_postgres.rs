use application::command::ports::id_mapping_port::{IdMappingRepository, MappingEntityType};
use async_trait::async_trait;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
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
        skuffen_id: SkuffenSakId,
        command: &Command,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let skuffen_id = Uuid::from(skuffen_id);
        if let Some(existing_skuffen_id) = self.hent_sak_id_fra_mapping(client_reference).await? {
            if Uuid::from(existing_skuffen_id) != skuffen_id {
                return Err(anyhow::anyhow!(
                    "client_reference is already mapped to a different skuffen_id"
                ));
            }
            let update_result = sqlx::query(
                r#"
                UPDATE id_mapping
                SET command_id = $1
                WHERE client_reference = $2 AND (command_id IS NULL OR command_id = $1)
                "#,
            )
            .bind(command_id)
            .bind(client_reference)
            .execute(&self.pool)
            .await?;
            if update_result.rows_affected() == 0 {
                let existing_command_id: Option<Option<Uuid>> = sqlx::query_scalar(
                    r#"
                    SELECT command_id
                    FROM id_mapping
                    WHERE client_reference = $1
                    "#,
                )
                .bind(client_reference)
                .fetch_optional(&self.pool)
                .await?;
                if existing_command_id.flatten().is_some() {
                    return Err(anyhow::anyhow!(
                        "client_reference is already registered for a different command_id"
                    ));
                }
                return Err(anyhow::anyhow!(
                    "Failed to update command_id for client_reference"
                ));
            }
            return Ok(());
        }

        let entity_type = match command {
            Command::OpprettSak(_) | Command::AvsluttSak(_) | Command::SettSaksansvarlig(_) => {
                "sak"
            }
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
            ON CONFLICT (client_reference) WHERE client_reference IS NOT NULL DO NOTHING
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
        skuffen_id: SkuffenDokumentId,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        let skuffen_id = Uuid::from(skuffen_id);
        if let Some(existing_skuffen_id) =
            self.hent_dokument_id_fra_mapping(client_reference).await?
        {
            if Uuid::from(existing_skuffen_id) != skuffen_id {
                return Err(anyhow::anyhow!(
                    "client_reference is already mapped to a different skuffen_id"
                ));
            }
            let update_result = sqlx::query(
                r#"
                UPDATE id_mapping
                SET command_id = $1
                WHERE client_reference = $2 AND (command_id IS NULL OR command_id = $1)
                "#,
            )
            .bind(command_id)
            .bind(client_reference)
            .execute(&self.pool)
            .await?;
            if update_result.rows_affected() == 0 {
                let existing_command_id: Option<Option<Uuid>> = sqlx::query_scalar(
                    r#"
                    SELECT command_id
                    FROM id_mapping
                    WHERE client_reference = $1
                    "#,
                )
                .bind(client_reference)
                .fetch_optional(&self.pool)
                .await?;
                if existing_command_id.flatten().is_some() {
                    return Err(anyhow::anyhow!(
                        "client_reference is already registered for a different command_id"
                    ));
                }
                return Err(anyhow::anyhow!(
                    "Failed to update command_id for client_reference"
                ));
            }
            return Ok(());
        }

        let entity_type = "dokument";

        sqlx::query(
            r#"
            INSERT INTO id_mapping (skuffen_id, entity_type, client_reference, arkiv_id, command_id)
            VALUES ($1, $2::entity_type, $3, $4, $5)
            ON CONFLICT (client_reference) WHERE client_reference IS NOT NULL DO NOTHING
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

    async fn oppdater_arkiv_id_for_client_reference(
        &self,
        client_reference: Uuid,
        arkiv_id: String,
    ) -> Result<(), anyhow::Error> {
        let result = sqlx::query(
            r#"
            UPDATE id_mapping
            SET arkiv_id = $1
            WHERE client_reference = $2
            "#,
        )
        .bind(arkiv_id)
        .bind(client_reference)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Fant ikke id_mapping for client_reference {}",
                client_reference
            ));
        }

        Ok(())
    }

    async fn hent_arkiv_id_fra_mapping(
        &self,
        skuffen_id: SkuffenSakId,
    ) -> Result<Option<String>, anyhow::Error> {
        let arkiv_id: Option<(Option<String>,)> = sqlx::query_as(
            r#"
            SELECT arkiv_id
            FROM id_mapping
            WHERE skuffen_id = $1
            "#,
        )
        .bind(Uuid::from(skuffen_id))
        .fetch_optional(&self.pool)
        .await?;

        Ok(arkiv_id.and_then(|(arkiv_id,)| arkiv_id))
    }

    async fn hent_sak_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error> {
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

        Ok(skuffen_id.map(SkuffenSakId::from))
    }

    async fn hent_journalpost_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenJournalpostId>, anyhow::Error> {
        let skuffen_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT skuffen_id
            FROM id_mapping
            WHERE client_reference = $1 AND entity_type = 'journalpost'
            "#,
        )
        .bind(client_reference)
        .fetch_optional(&self.pool)
        .await?;

        Ok(skuffen_id.map(SkuffenJournalpostId::from))
    }

    async fn hent_dokument_id_fra_mapping(
        &self,
        client_reference: Uuid,
    ) -> Result<Option<SkuffenDokumentId>, anyhow::Error> {
        let skuffen_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT skuffen_id
            FROM id_mapping
            WHERE client_reference = $1 AND entity_type = 'dokument'
            "#,
        )
        .bind(client_reference)
        .fetch_optional(&self.pool)
        .await?;

        Ok(skuffen_id.map(SkuffenDokumentId::from))
    }

    async fn hent_sak_id_fra_arkiv_id_i_mapping(
        &self,
        arkiv_id: &str,
    ) -> Result<Option<SkuffenSakId>, anyhow::Error> {
        let skuffen_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT skuffen_id
            FROM id_mapping
            WHERE arkiv_id = $1 AND entity_type = 'sak'
            "#,
        )
        .bind(arkiv_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(skuffen_id.map(SkuffenSakId::from))
    }

    async fn hent_eller_opprett_skuffen_id_for_arkiv_id(
        &self,
        entity_type: MappingEntityType,
        arkiv_id: &str,
    ) -> Result<SkuffenSakId, anyhow::Error> {
        let entity_type_code = entity_type.as_code();
        let existing: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT skuffen_id
            FROM id_mapping
            WHERE entity_type = $1::entity_type AND arkiv_id = $2
            "#,
        )
        .bind(entity_type_code)
        .bind(arkiv_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(skuffen_id) = existing {
            return Ok(SkuffenSakId::from(skuffen_id));
        }

        let skuffen_id = Uuid::now_v7();
        sqlx::query(
            r#"
            INSERT INTO id_mapping (skuffen_id, entity_type, client_reference, arkiv_id)
            VALUES ($1, $2::entity_type, NULL, $3)
            ON CONFLICT (entity_type, arkiv_id) WHERE arkiv_id IS NOT NULL DO NOTHING
            "#,
        )
        .bind(skuffen_id)
        .bind(entity_type_code)
        .bind(arkiv_id)
        .execute(&self.pool)
        .await?;

        // We must read the row back after the insert attempt. If another worker inserted the
        // same (entity_type, arkiv_id) first, ON CONFLICT DO NOTHING discards our generated
        // skuffen_id and the row already in the table becomes the canonical mapping.
        let skuffen_id: Option<Uuid> = sqlx::query_scalar(
            r#"
            SELECT skuffen_id
            FROM id_mapping
            WHERE entity_type = $1::entity_type AND arkiv_id = $2
            "#,
        )
        .bind(entity_type_code)
        .bind(arkiv_id)
        .fetch_optional(&self.pool)
        .await?;

        skuffen_id.map(SkuffenSakId::from).ok_or_else(|| {
            anyhow::anyhow!(
                "Fant ikke id_mapping for entity_type {} arkiv_id {}",
                entity_type_code,
                arkiv_id
            )
        })
    }

    async fn delete_arkiv_mapping(
        &self,
        entity_type: MappingEntityType,
        arkiv_id: &str,
    ) -> Result<(), anyhow::Error> {
        let entity_type_code = entity_type.as_code();
        sqlx::query(
            r#"
            DELETE FROM id_mapping
            WHERE entity_type = $1::entity_type AND arkiv_id = $2
            "#,
        )
        .bind(entity_type_code)
        .bind(arkiv_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
