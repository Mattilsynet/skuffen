use anyhow::{Context, Result};
use application::command::ports::command_port::{CommandRepository, Mottaksresultat};
use application::command::services::ingest_command::command_type;
use application::command::{Command, CommandEnvelope};
use async_trait::async_trait;
use lib_sql::database_config::DbPool;
use uuid::Uuid;

/// Mottaksjournal og idempotency-hovedbok.
///
/// Idempotency-nøkkelen er `dispatchet_at`, ikke radens eksistens
/// (SKU-0016 R11). Raden skrives ved mottak; milepælen settes av
/// [`marker_dispatchet`] først etter at dispatch faktisk lyktes.
pub struct PostgresCommandRepository {
    pool: DbPool,
}

impl PostgresCommandRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CommandRepository for PostgresCommandRepository {
    async fn registrer_mottatt(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<Mottaksresultat> {
        let command_type = command_type(&envelope.payload).as_code();

        // Én tur-retur: sett inn hvis ny, og les alltid ut dispatch-milepælen.
        // `er_ny` skiller førstegangs mottak fra et gjenopptatt forsøk.
        let (er_ny, dispatchet_at): (bool, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as(
            r#"
            WITH ny AS (
                INSERT INTO command (command_id, correlation_id, command_type)
                VALUES ($1, $2, $3)
                ON CONFLICT (command_id) DO NOTHING
                RETURNING dispatchet_at
            )
            SELECT true, dispatchet_at FROM ny
            UNION ALL
            SELECT false, dispatchet_at FROM command WHERE command_id = $1
            LIMIT 1
            "#,
        )
        .bind(envelope.command_id)
        .bind(envelope.correlation_id)
        .bind(command_type)
        .fetch_one(&self.pool)
        .await
        .context("failed to record command receipt")?;

        Ok(match (dispatchet_at, er_ny) {
            (Some(_), _) => Mottaksresultat::AlleredeDispatchet,
            (None, true) => Mottaksresultat::Ny,
            (None, false) => Mottaksresultat::MottattIkkeDispatchet,
        })
    }

    async fn marker_dispatchet(&self, command_id: Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE command SET dispatchet_at = now() WHERE command_id = $1 AND dispatchet_at IS NULL",
        )
        .bind(command_id)
        .execute(&self.pool)
        .await
        .context("failed to mark command dispatched")?;
        Ok(())
    }
}
