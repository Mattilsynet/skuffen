//! Admin-forkasting mot PostgreSQL (SKU-0024).

use anyhow::Context;
use application::admin::ports::admin_forkast_port::{AdminForkastRepository, ForkastFeil};
use async_trait::async_trait;
use lib_sql::database_config::DbPool;
use uuid::Uuid;

pub struct PostgresAdminForkastRepository {
    pool: DbPool,
}

impl PostgresAdminForkastRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AdminForkastRepository for PostgresAdminForkastRepository {
    async fn forkast(&self, command_ids: &[Uuid]) -> Result<(), ForkastFeil> {
        let mut tx = self.pool.begin().await.context("failed to begin")?;

        let kjente: Vec<Uuid> =
            sqlx::query_scalar("SELECT command_id FROM command WHERE command_id = ANY($1)")
                .bind(command_ids)
                .fetch_all(&mut *tx)
                .await
                .context("failed to load commands")?;
        let ukjente: Vec<Uuid> = command_ids
            .iter()
            .filter(|id| !kjente.contains(id))
            .copied()
            .collect();
        if !ukjente.is_empty() {
            return Err(ForkastFeil::CommandNotFound(ukjente));
        }

        // Fullført (minst én operasjon, alle `ok`) skrives ikke om.
        // Blokkerte avslutninger på sakene vurderes straks, ikke om 30 s.
        sqlx::query(
            r#"
            WITH forkastet AS (
                UPDATE command c SET forkastet_at = now()
                WHERE c.command_id = ANY($1)
                  AND c.forkastet_at IS NULL
                  AND NOT (EXISTS (SELECT 1 FROM operasjon o WHERE o.command_id = c.command_id)
                           AND NOT EXISTS (SELECT 1 FROM operasjon o
                                           WHERE o.command_id = c.command_id
                                             AND o.status <> 'ok'))
                RETURNING c.sak_id
            )
            UPDATE operasjon SET neste_forsok_at = now()
            WHERE sak_id IN (SELECT sak_id FROM forkastet)
              AND operasjonstype = 'avslutt_sak'
              AND status = 'blokkert'
            "#,
        )
        .bind(command_ids)
        .execute(&mut *tx)
        .await
        .context("failed to forkast commands")?;

        tx.commit().await.context("failed to commit forkast")?;
        Ok(())
    }
}
