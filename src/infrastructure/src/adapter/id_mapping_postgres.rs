use application::ports::id_mapping_port::IdMappingRepository;
use async_trait::async_trait;
use sqlx::postgres::PgPool;
use uuid::Uuid;

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
    async fn register_mapping(
        &self,
        command_id: Uuid,
        skuffen_id: Uuid,
        entity_type: String,
        arkiv_id: Option<String>,
    ) -> Result<(), anyhow::Error> {
        // Insert into id_mapping.
        // client_reference corresponds to command_id in this context (unique request ID).
        // On conflict (client_reference): do nothing (idempotent), but we might want to verify if other fields match?
        // For MVP, ON CONFLICT DO NOTHING is safest for idempotency.

        sqlx::query(
            r#"
            INSERT INTO id_mapping (skuffen_id, entity_type, client_reference, arkiv_id, command_id)
            VALUES ($1, $2::entity_type, $3, $4, $5)
            ON CONFLICT (client_reference) DO NOTHING
            "#,
        )
        .bind(skuffen_id)
        .bind(entity_type.clone()) // Or just string if Postgres casts it, but explicit cast in SQL helps
        .bind(command_id)
        .bind(arkiv_id)
        .bind(command_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
