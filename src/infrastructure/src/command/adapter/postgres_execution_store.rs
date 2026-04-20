use application::command::ports::command_execution_port::{
    CommandExecutionRepository, EksekveringKommando, EksekveringsregistreringResultat,
    NyKommandoEksekvering,
};
use async_trait::async_trait;
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::typer::CommandTypeCode;
use sqlx::postgres::PgPool;
use sqlx::types::chrono;
use sqlx::{Postgres, pool::PoolConnection};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresExecutionStore {
    pool: PgPool,
    executor_lock_connection: Arc<Mutex<Option<PoolConnection<Postgres>>>>,
}

impl PostgresExecutionStore {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            executor_lock_connection: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl CommandExecutionRepository for PostgresExecutionStore {
    async fn try_acquire_executor_lock(&self, _executor_id: &str) -> Result<bool, anyhow::Error> {
        let mut lock = self.executor_lock_connection.lock().await;
        if lock.is_some() {
            return Ok(true);
        }

        let mut conn = self.pool.acquire().await?;
        let acquired: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock(84712631)")
            .fetch_one(&mut *conn)
            .await?;

        if acquired {
            *lock = Some(conn);
        }

        Ok(acquired)
    }

    async fn opprett(
        &self,
        ny: NyKommandoEksekvering,
    ) -> Result<EksekveringsregistreringResultat, anyhow::Error> {
        let mut tx = self.pool.begin().await?;

        let payload = serde_json::to_value(&ny.envelope)?;
        let result = sqlx::query(
            r#"
            INSERT INTO command_execution (
                command_id, correlation_id, payload, command_type, sak_id, journalpost_id,
                status, attempt_no, retry_ready_at,
                last_detail, utfores_venter_publisert_at, finished_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,0,NULL,$8,NULL,$9)
            ON CONFLICT (command_id) DO NOTHING
            "#,
        )
        .bind(ny.envelope.command_id)
        .bind(ny.envelope.correlation_id)
        .bind(payload)
        .bind(command_type_code(ny.command_type))
        .bind(ny.sak_id.map(Uuid::from))
        .bind(ny.journalpost_id.map(Uuid::from))
        .bind(ny.status.as_db_code())
        .bind(ny.last_detail.clone())
        .bind(
            matches!(
                ny.status,
                domain::eksekvering::execution::EksekveringStatus::Feil
            )
            .then(chrono::Utc::now),
        )
        .execute(&mut *tx)
        .await?;

        let registrering = if result.rows_affected() > 0 {
            EksekveringsregistreringResultat::Nyregistrert
        } else {
            let published_at: Option<Option<chrono::DateTime<chrono::Utc>>> = sqlx::query_scalar(
                r#"
                SELECT utfores_venter_publisert_at
                FROM command_execution
                WHERE command_id = $1
                "#,
            )
            .bind(ny.envelope.command_id)
            .fetch_optional(&mut *tx)
            .await?;

            match published_at {
                Some(Some(_)) => EksekveringsregistreringResultat::EksisterteMedVenterPublisert,
                Some(None) => EksekveringsregistreringResultat::EksisterteUtenVenterPublisert,
                None => {
                    return Err(anyhow::anyhow!(
                        "Fant ikke command_execution etter opprett for command_id {}",
                        ny.envelope.command_id
                    ));
                }
            }
        };

        tx.commit().await?;
        Ok(registrering)
    }

    async fn marker_utfores_venter_publisert(&self, command_id: Uuid) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE command_execution
            SET utfores_venter_publisert_at = COALESCE(utfores_venter_publisert_at, now()),
                updated_at = now()
            WHERE command_id = $1
            "#,
        )
        .bind(command_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn hent_neste_kjorbare(&self) -> Result<Option<EksekveringKommando>, anyhow::Error> {
        let row: Option<(Uuid, serde_json::Value, i32, bool)> = sqlx::query_as(
            r#"
            SELECT command_id, payload, attempt_no, utfores_venter_publisert_at IS NOT NULL
            FROM command_execution
            WHERE status = 'klar'
               OR (status = 'retry_venter' AND retry_ready_at <= now())
            ORDER BY created_at
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        row.map(
            |(command_id, payload, attempt_no, utfores_venter_publisert)| {
                let envelope = serde_json::from_value(payload)?;
                Ok(EksekveringKommando {
                    command_id,
                    envelope,
                    attempt_no,
                    utfores_venter_publisert,
                })
            },
        )
        .transpose()
    }

    async fn marker_kjorer(&self, command_id: Uuid) -> Result<i32, anyhow::Error> {
        let attempt_no: Option<i32> = sqlx::query_scalar(
            r#"
            UPDATE command_execution
            SET status = 'kjorer',
                attempt_no = attempt_no + 1,
                retry_ready_at = NULL,
                updated_at = now(),
                started_at = COALESCE(started_at, now())
            WHERE command_id = $1
              AND status IN ('klar', 'retry_venter')
            RETURNING attempt_no
            "#,
        )
        .bind(command_id)
        .fetch_optional(&self.pool)
        .await?;

        attempt_no
            .ok_or_else(|| anyhow::anyhow!("Kunne ikke markere command {command_id} som kjorer"))
    }

    async fn registrer_forsok(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        executor_id: &str,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            INSERT INTO command_execution_attempt (command_id, attempt_no, executor_id)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .bind(executor_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn marker_ok(&self, command_id: Uuid, attempt_no: i32) -> Result<(), anyhow::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
            UPDATE command_execution
            SET status = 'ok',
                last_detail = NULL,
                updated_at = now(),
                finished_at = now()
            WHERE command_id = $1
              AND status = 'kjorer'
              AND attempt_no = $2
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Kunne ikke markere command {} som ok for attempt {}",
                command_id,
                attempt_no
            ));
        }
        avslutt_forsok(&mut tx, command_id, attempt_no, "ok", None).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn marker_retry_venter(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        detalj: &str,
        retry_ready_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), anyhow::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
            UPDATE command_execution
            SET status = 'retry_venter',
                retry_ready_at = $3,
                last_detail = $4,
                updated_at = now(),
                finished_at = NULL
            WHERE command_id = $1
              AND status = 'kjorer'
              AND attempt_no = $2
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .bind(retry_ready_at)
        .bind(detalj)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Kunne ikke markere command {} som retry_venter for attempt {}",
                command_id,
                attempt_no
            ));
        }
        avslutt_forsok(
            &mut tx,
            command_id,
            attempt_no,
            "retry_venter",
            Some(detalj),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn marker_blokkert_venter(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
            UPDATE command_execution
            SET status = 'blokkert_venter',
                retry_ready_at = NULL,
                last_detail = $3,
                updated_at = now(),
                finished_at = NULL
            WHERE command_id = $1
              AND status = 'kjorer'
              AND attempt_no = $2
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .bind(detalj)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Kunne ikke markere command {} som blokkert_venter for attempt {}",
                command_id,
                attempt_no
            ));
        }
        avslutt_forsok(
            &mut tx,
            command_id,
            attempt_no,
            "blokkert_venter",
            Some(detalj),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn marker_feil(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
            UPDATE command_execution
            SET status = 'feil',
                retry_ready_at = NULL,
                last_detail = $3,
                updated_at = now(),
                finished_at = now()
            WHERE command_id = $1
              AND status = 'kjorer'
              AND attempt_no = $2
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .bind(detalj)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Kunne ikke markere command {} som feil for attempt {}",
                command_id,
                attempt_no
            ));
        }
        avslutt_forsok(&mut tx, command_id, attempt_no, "feil", Some(detalj)).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn marker_forsok_avbrutt(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE command_execution_attempt
            SET outcome = 'avbrutt',
                detail = $3,
                finished_at = now()
            WHERE command_id = $1
              AND attempt_no = $2
              AND finished_at IS NULL
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .bind(detalj)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn hent_blokkert_venter_for_sak(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error> {
        let rows: Vec<(Uuid, serde_json::Value, i32, bool)> = sqlx::query_as(
            r#"
            SELECT command_id, payload, attempt_no, utfores_venter_publisert_at IS NOT NULL
            FROM command_execution
            WHERE status = 'blokkert_venter' AND sak_id = $1
            ORDER BY created_at
            "#,
        )
        .bind(Uuid::from(sak_id))
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::with_capacity(rows.len());
        for (command_id, payload, attempt_no, utfores_venter_publisert) in rows {
            result.push(EksekveringKommando {
                command_id,
                envelope: serde_json::from_value(payload)?,
                attempt_no,
                utfores_venter_publisert,
            });
        }
        Ok(result)
    }

    async fn marker_blokkert_venter_til_klar(&self, command_id: Uuid) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE command_execution
            SET status = 'klar',
                retry_ready_at = NULL,
                last_detail = NULL,
                updated_at = now(),
                finished_at = NULL
            WHERE command_id = $1
              AND status = 'blokkert_venter'
            "#,
        )
        .bind(command_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn oppdater_til_klar(&self, command_id: Uuid) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE command_execution
            SET status = 'klar',
                retry_ready_at = NULL,
                last_detail = NULL,
                updated_at = now(),
                finished_at = NULL
            WHERE command_id = $1
            "#,
        )
        .bind(command_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn oppdater_til_feil(&self, command_id: Uuid, detalj: &str) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE command_execution
            SET status = 'feil',
                retry_ready_at = NULL,
                last_detail = $2,
                updated_at = now(),
                finished_at = now()
            WHERE command_id = $1
            "#,
        )
        .bind(command_id)
        .bind(detalj)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn reset_kjorer_til_klar(&self) -> Result<u64, anyhow::Error> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"
            UPDATE command_execution_attempt
            SET outcome = 'avbrutt',
                detail = 'executor restarted before command finished',
                finished_at = now()
            WHERE finished_at IS NULL
            "#,
        )
        .execute(&mut *tx)
        .await?;

        let result = sqlx::query(
            r#"
            UPDATE command_execution
            SET status = 'klar',
                retry_ready_at = NULL,
                updated_at = now(),
                finished_at = NULL
            WHERE status = 'kjorer'
            "#,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(result.rows_affected())
    }
}

fn command_type_code(command_type: CommandTypeCode) -> &'static str {
    command_type.as_code()
}

async fn avslutt_forsok(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    command_id: Uuid,
    attempt_no: i32,
    outcome: &str,
    detail: Option<&str>,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        r#"
        UPDATE command_execution_attempt
        SET outcome = $3,
            detail = $4,
            finished_at = now()
        WHERE command_id = $1
          AND attempt_no = $2
          AND finished_at IS NULL
        "#,
    )
    .bind(command_id)
    .bind(attempt_no)
    .bind(outcome)
    .bind(detail)
    .execute(&mut **tx)
    .await?;

    Ok(())
}
