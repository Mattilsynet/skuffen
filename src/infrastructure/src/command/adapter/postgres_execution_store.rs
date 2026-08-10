use application::command::ports::command_execution_port::{
    CommandExecutionRepository, EksekveringKommando, EksekveringsregistreringResultat,
    NyKommandoEksekvering,
};
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope as WireCommandEnvelope};

use crate::command::wire_mapper::{map_application_envelope_to_wire, map_wire_envelope};
use async_trait::async_trait;
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::typer::CommandTypeCode;
use sqlx::postgres::PgPool;
use sqlx::types::chrono;
use sqlx::{Postgres, pool::PoolConnection};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
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

        let wire_envelope = map_application_envelope_to_wire(&ny.envelope)?;
        let payload = serde_json::to_value(&wire_envelope)?;
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

        if matches!(registrering, EksekveringsregistreringResultat::Nyregistrert) {
            let log_context = ExecutionLogContext {
                correlation_id: ny.envelope.correlation_id,
                command_type: command_type_code(ny.command_type).to_string(),
                sak_id: ny.sak_id.map(Uuid::from),
                journalpost_id: ny.journalpost_id.map(Uuid::from),
            };
            log_command_execution_outcome(
                ny.envelope.command_id,
                None,
                ny.status.as_db_code(),
                ny.last_detail.as_deref(),
                &log_context,
            );
        }

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
                let envelope = payload_to_application_envelope(payload)?;
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

    async fn marker_klar(&self, command_id: Uuid, attempt_no: i32) -> Result<(), anyhow::Error> {
        let mut tx = self.pool.begin().await?;
        let log_context = sqlx::query_as::<_, ExecutionLogContext>(
            r#"
            UPDATE command_execution
            SET status = 'klar',
                retry_ready_at = NULL,
                last_detail = NULL,
                updated_at = now(),
                finished_at = NULL
            WHERE command_id = $1
              AND status = 'kjorer'
              AND attempt_no = $2
            RETURNING correlation_id, command_type, sak_id, journalpost_id
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(log_context) = log_context else {
            return Err(anyhow::anyhow!(
                "Kunne ikke markere command {} som klar for attempt {}",
                command_id,
                attempt_no
            ));
        };
        avslutt_forsok(&mut tx, command_id, attempt_no, "klar", None).await?;
        tx.commit().await?;
        log_command_execution_outcome(command_id, Some(attempt_no), "klar", None, &log_context);
        Ok(())
    }

    async fn marker_ok(&self, command_id: Uuid, attempt_no: i32) -> Result<(), anyhow::Error> {
        let mut tx = self.pool.begin().await?;
        let log_context = sqlx::query_as::<_, ExecutionLogContext>(
            r#"
            UPDATE command_execution
            SET status = 'ok',
                last_detail = NULL,
                updated_at = now(),
                finished_at = now()
            WHERE command_id = $1
              AND status = 'kjorer'
              AND attempt_no = $2
            RETURNING correlation_id, command_type, sak_id, journalpost_id
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(log_context) = log_context else {
            return Err(anyhow::anyhow!(
                "Kunne ikke markere command {} som ok for attempt {}",
                command_id,
                attempt_no
            ));
        };
        avslutt_forsok(&mut tx, command_id, attempt_no, "ok", None).await?;
        tx.commit().await?;
        log_command_execution_outcome(command_id, Some(attempt_no), "ok", None, &log_context);
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
        let log_context = sqlx::query_as::<_, ExecutionLogContext>(
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
            RETURNING correlation_id, command_type, sak_id, journalpost_id
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .bind(retry_ready_at)
        .bind(detalj)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(log_context) = log_context else {
            return Err(anyhow::anyhow!(
                "Kunne ikke markere command {} som retry_venter for attempt {}",
                command_id,
                attempt_no
            ));
        };
        avslutt_forsok(
            &mut tx,
            command_id,
            attempt_no,
            "retry_venter",
            Some(detalj),
        )
        .await?;
        tx.commit().await?;
        log_command_execution_outcome(
            command_id,
            Some(attempt_no),
            "retry_venter",
            Some(detalj),
            &log_context,
        );
        Ok(())
    }

    async fn marker_blokkert_venter(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        let mut tx = self.pool.begin().await?;
        let log_context = sqlx::query_as::<_, ExecutionLogContext>(
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
            RETURNING correlation_id, command_type, sak_id, journalpost_id
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .bind(detalj)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(log_context) = log_context else {
            return Err(anyhow::anyhow!(
                "Kunne ikke markere command {} som blokkert_venter for attempt {}",
                command_id,
                attempt_no
            ));
        };
        avslutt_forsok(
            &mut tx,
            command_id,
            attempt_no,
            "blokkert_venter",
            Some(detalj),
        )
        .await?;
        tx.commit().await?;
        log_command_execution_outcome(
            command_id,
            Some(attempt_no),
            "blokkert_venter",
            Some(detalj),
            &log_context,
        );
        Ok(())
    }

    async fn marker_feil(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        let mut tx = self.pool.begin().await?;
        let log_context = sqlx::query_as::<_, ExecutionLogContext>(
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
            RETURNING correlation_id, command_type, sak_id, journalpost_id
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .bind(detalj)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(log_context) = log_context else {
            return Err(anyhow::anyhow!(
                "Kunne ikke markere command {} som feil for attempt {}",
                command_id,
                attempt_no
            ));
        };
        avslutt_forsok(&mut tx, command_id, attempt_no, "feil", Some(detalj)).await?;
        tx.commit().await?;
        log_command_execution_outcome(
            command_id,
            Some(attempt_no),
            "feil",
            Some(detalj),
            &log_context,
        );
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
                envelope: payload_to_application_envelope(payload)?,
                attempt_no,
                utfores_venter_publisert,
            });
        }
        Ok(result)
    }

    async fn oppdater_til_klar(&self, command_id: Uuid) -> Result<(), anyhow::Error> {
        let log_context = sqlx::query_as::<_, ExecutionLogContext>(
            r#"
            UPDATE command_execution
            SET status = 'klar',
                retry_ready_at = NULL,
                last_detail = NULL,
                updated_at = now(),
                finished_at = NULL
            WHERE command_id = $1
              AND status = 'blokkert_venter'
            RETURNING correlation_id, command_type, sak_id, journalpost_id
            "#,
        )
        .bind(command_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(log_context) = log_context {
            log_command_execution_outcome(command_id, None, "klar", None, &log_context);
        }

        Ok(())
    }

    async fn oppdater_blokkert_detail(
        &self,
        command_id: Uuid,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        let log_context = sqlx::query_as::<_, ExecutionLogContext>(
            r#"
            UPDATE command_execution
            SET last_detail = $2,
                updated_at = now()
            WHERE command_id = $1
              AND status = 'blokkert_venter'
            RETURNING correlation_id, command_type, sak_id, journalpost_id
            "#,
        )
        .bind(command_id)
        .bind(detalj)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(log_context) = log_context {
            log_command_execution_outcome(
                command_id,
                None,
                "blokkert_venter",
                Some(detalj),
                &log_context,
            );
        }

        Ok(())
    }

    async fn oppdater_til_feil(&self, command_id: Uuid, detalj: &str) -> Result<(), anyhow::Error> {
        let log_context = sqlx::query_as::<_, ExecutionLogContext>(
            r#"
            UPDATE command_execution
            SET status = 'feil',
                retry_ready_at = NULL,
                last_detail = $2,
                updated_at = now(),
                finished_at = now()
            WHERE command_id = $1
            RETURNING correlation_id, command_type, sak_id, journalpost_id
            "#,
        )
        .bind(command_id)
        .bind(detalj)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(log_context) = log_context {
            log_command_execution_outcome(command_id, None, "feil", Some(detalj), &log_context);
        }

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

#[derive(sqlx::FromRow)]
struct ExecutionLogContext {
    correlation_id: Option<Uuid>,
    command_type: String,
    sak_id: Option<Uuid>,
    journalpost_id: Option<Uuid>,
}

fn log_command_execution_outcome(
    command_id: Uuid,
    attempt_no: Option<i32>,
    outcome: &'static str,
    detail: Option<&str>,
    context: &ExecutionLogContext,
) {
    let correlation_id = format_optional_uuid(context.correlation_id);
    let sak_id = format_optional_uuid(context.sak_id);
    let journalpost_id = format_optional_uuid(context.journalpost_id);
    let attempt_no = attempt_no
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let sanitized_detail = detail.and_then(sanitize_log_detail);
    let last_error = sanitized_detail.as_deref().unwrap_or("omitted");
    let error_category = detail.map(error_category_for_detail).unwrap_or("none");
    let headline = command_outcome_headline(outcome, error_category);

    match outcome {
        "ok" => info!(
            event = "command_execution_outcome",
            command_id = %command_id,
            correlation_id = %correlation_id,
            command_type = %context.command_type,
            attempt_no = %attempt_no,
            outcome,
            sak_id = %sak_id,
            journalpost_id = %journalpost_id,
            diagnostic_code = %error_category,
            error_category = %error_category,
            "{}", headline
        ),
        "klar" => info!(
            event = "command_execution_outcome",
            command_id = %command_id,
            correlation_id = %correlation_id,
            command_type = %context.command_type,
            attempt_no = %attempt_no,
            outcome,
            sak_id = %sak_id,
            journalpost_id = %journalpost_id,
            detail = %last_error,
            last_error = %last_error,
            error_classification = "none",
            diagnostic_code = %error_category,
            error_category = %error_category,
            "{}", headline
        ),
        "feil" => error!(
            event = "command_execution_outcome",
            command_id = %command_id,
            correlation_id = %correlation_id,
            command_type = %context.command_type,
            attempt_no = %attempt_no,
            outcome,
            sak_id = %sak_id,
            journalpost_id = %journalpost_id,
            detail = %last_error,
            last_error = %last_error,
            error_classification = "irrecoverable",
            diagnostic_code = %error_category,
            error_category = %error_category,
            "{}", headline
        ),
        "retry_venter" => warn!(
            event = "command_execution_outcome",
            command_id = %command_id,
            correlation_id = %correlation_id,
            command_type = %context.command_type,
            attempt_no = %attempt_no,
            outcome,
            sak_id = %sak_id,
            journalpost_id = %journalpost_id,
            detail = %last_error,
            last_error = %last_error,
            error_classification = "recoverable",
            diagnostic_code = %error_category,
            error_category = %error_category,
            "{}", headline
        ),
        "blokkert_venter" => warn!(
            event = "command_execution_outcome",
            command_id = %command_id,
            correlation_id = %correlation_id,
            command_type = %context.command_type,
            attempt_no = %attempt_no,
            outcome,
            sak_id = %sak_id,
            journalpost_id = %journalpost_id,
            detail = %last_error,
            last_error = %last_error,
            error_classification = "blocked",
            diagnostic_code = %error_category,
            error_category = %error_category,
            "{}", headline
        ),
        _ => warn!(
            event = "command_execution_outcome",
            command_id = %command_id,
            correlation_id = %correlation_id,
            command_type = %context.command_type,
            attempt_no = %attempt_no,
            outcome,
            sak_id = %sak_id,
            journalpost_id = %journalpost_id,
            detail = %last_error,
            last_error = %last_error,
            error_classification = "unknown",
            diagnostic_code = %error_category,
            error_category = %error_category,
            "{}", headline
        ),
    }
}

fn command_outcome_headline(outcome: &str, diagnostic_code: &str) -> String {
    match (outcome, diagnostic_code) {
        ("ok", _) => "command ok".to_string(),
        ("klar", _) => "command ready".to_string(),
        ("retry_venter", "none") => "command retrying".to_string(),
        ("retry_venter", code) => format!("command retrying: {code}"),
        ("blokkert_venter", "none") => "command blocked".to_string(),
        ("blokkert_venter", code) => format!("command blocked: {code}"),
        ("feil", "none") => "command failed".to_string(),
        ("feil", code) => format!("command failed: {code}"),
        (other, "none") => format!("command outcome: {other}"),
        (other, code) => format!("command outcome: {other}: {code}"),
    }
}

fn format_optional_uuid(id: Option<Uuid>) -> String {
    id.map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn sanitize_log_detail(detail: &str) -> Option<String> {
    const MAX_LOG_DETAIL_CHARS: usize = 500;

    let stripped = detail
        .replace("sikri_recoverability=irrecoverable", "")
        .replace("sikri_recoverability=recoverable", "");
    let normalized = stripped
        .chars()
        .filter(|c| !c.is_control() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if normalized.is_empty() {
        return None;
    }

    let redacted = redact_sensitive_log_tokens(&normalized);
    let payload_stripped = strip_embedded_payload(&redacted);
    let trimmed = payload_stripped.trim();
    if trimmed.is_empty() {
        return None;
    }

    Some(truncate_log_detail(trimmed, MAX_LOG_DETAIL_CHARS))
}

fn error_category_for_detail(detail: &str) -> &'static str {
    if detail.starts_with("invalid_reason=journalpost_mangler") {
        "journalpost_mangler"
    } else if detail.starts_with("invalid_reason=dokument_feilet_permanent") {
        "dokument_feilet_permanent"
    } else if detail.starts_with("invalid_reason=journalpost_feilet_permanent") {
        "journalpost_feilet_permanent"
    } else if detail.starts_with("invalid_reason=sak_feilet_permanent") {
        "sak_feilet_permanent"
    } else if detail.starts_with("invalid_reason=journalpost_type_mismatch") {
        "journalpost_type_mismatch"
    } else if detail.starts_with("blocked_reason=entity_missing") {
        "entity_missing"
    } else if detail.starts_with("blocked_reason=saksnummer_mangler") {
        "saksnummer_mangler"
    } else if detail.starts_with("blocked_reason=saksansvarlig_ikke_satt") {
        "saksansvarlig_ikke_satt"
    } else if detail.starts_with("blocked_reason=journalposter_ikke_ferdige") {
        "journalposter_ikke_ferdige"
    } else if detail.starts_with("blocked_reason=felter_ikke_klare") {
        "felter_ikke_klare"
    } else if detail.starts_with("blocked_reason=journalpost_tilstand_uavklart") {
        "journalpost_tilstand_uavklart"
    } else if detail.starts_with("blocked_reason=permanent_feil") {
        "permanent_feil"
    } else if detail.starts_with("html2pdf_auth_failed") {
        "html2pdf_auth_failed"
    } else if detail.starts_with("html2pdf_client_error") {
        "html2pdf_client_error"
    } else if detail.starts_with("html2pdf_server_error") {
        "html2pdf_server_error"
    } else if detail.starts_with("html2pdf_request_failed") {
        "html2pdf_request_failed"
    } else if detail.starts_with("html2pdf_response_read_failed") {
        "html2pdf_response_read_failed"
    } else if detail.starts_with("render_dokument_mangler") {
        "render_dokument_mangler"
    } else if detail.starts_with("render_journalpost_mangler") {
        "render_journalpost_mangler"
    } else if detail.starts_with("render_ikke_html_template") {
        "render_ikke_html_template"
    } else if detail.starts_with("render_saksnummer_mangler") {
        "render_saksnummer_mangler"
    } else if detail.starts_with("render_html_mal_mangler") {
        "render_html_mal_mangler"
    } else if detail.starts_with("render_html_mal_lager_unavailable") {
        "render_html_mal_lager_unavailable"
    } else if detail.starts_with("render_token_substitution_failed") {
        "render_token_substitution_failed"
    } else if detail.starts_with("rendered_dokument_save_failed") {
        "rendered_dokument_save_failed"
    } else if detail.starts_with("render_state_update_failed") {
        "render_state_update_failed"
    } else if detail.starts_with("arkivmapping_dokument_fact_mangler") {
        "arkivmapping_dokument_fact_mangler"
    } else if detail.starts_with("arkivmapping_rendered_dokument_mangler") {
        "arkivmapping_rendered_dokument_mangler"
    } else if detail.starts_with("arkivmapping_dokumentform_mismatch") {
        "arkivmapping_dokumentform_mismatch"
    } else if detail.contains("Ugyldig kommando") {
        "invalid_command"
    } else {
        "execution_error"
    }
}

fn redact_sensitive_log_tokens(detail: &str) -> String {
    let mut redacted = Vec::new();
    let mut redacted_remaining = 0;

    for token in detail.split_whitespace() {
        if redacted_remaining > 0 {
            redacted.push("redacted".to_string());
            redacted_remaining -= 1;
            continue;
        }

        let lower = token.to_ascii_lowercase();
        if is_sensitive_log_token(&lower) {
            redacted.push(redact_sensitive_log_token(token));
            redacted_remaining = sensitive_following_token_count(token, &lower);
        } else {
            redacted.push(token.to_string());
        }
    }

    redacted.join(" ")
}

fn is_sensitive_log_token(lower: &str) -> bool {
    lower.contains("authorization")
        || lower == "bearer"
        || lower.starts_with("bearer=")
        || lower == "basic"
        || lower.starts_with("basic=")
        || lower.contains("password")
        || lower.contains("passwd")
        || lower.contains("credential")
        || lower.contains("token")
        || lower.contains("api_key")
        || lower.contains("apikey")
        || lower.contains("x-api-key")
        || lower.contains("secret")
}

fn sensitive_following_token_count(token: &str, lower: &str) -> usize {
    if lower == "bearer" || lower == "basic" {
        1
    } else if lower.contains("authorization") && token.ends_with(':') {
        2
    } else if token.ends_with(':') || token == "=" {
        1
    } else {
        0
    }
}

fn redact_sensitive_log_token(token: &str) -> String {
    token
        .find(['=', ':'])
        .map(|index| format!("{}redacted", &token[..=index]))
        .unwrap_or_else(|| "redacted".to_string())
}

fn strip_embedded_payload(detail: &str) -> String {
    let Some(index) = detail.find('{') else {
        return detail.to_string();
    };

    let prefix = detail[..index].trim_end();
    if prefix.is_empty() {
        "[payload stripped]".to_string()
    } else {
        format!("{prefix} [payload stripped]")
    }
}

fn truncate_log_detail(detail: &str, max_chars: usize) -> String {
    let mut value: String = detail.chars().take(max_chars).collect();
    if detail.chars().count() > max_chars {
        value.push_str("...");
    }
    value
}

fn payload_to_application_envelope(
    payload: serde_json::Value,
) -> Result<application::command::CommandEnvelope<application::command::Command>, anyhow::Error> {
    let wire_envelope: WireCommandEnvelope<Command> = serde_json::from_value(payload)?;
    Ok(map_wire_envelope(wire_envelope))
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

#[cfg(test)]
mod tests {
    use super::error_category_for_detail;
    use domain::eksekvering::tilstand::{DomainViolation, DomainViolation::*};
    use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
    use lib_schemas::skuffen::command::journalpost::{
        JournalpostCommon, Korrespondansepart, OpprettInngåendeJournalpost,
        OpprettInterntNotatJournalpost, OpprettUtgåendeJournalpost, Parttype,
    };
    use lib_schemas::skuffen::command::sak::{Arkivdel, AvsluttSak, OpprettSak, SettSaksansvarlig};
    use lib_schemas::skuffen::dokument::{Dokument, Dokumentform};
    use lib_schemas::skuffen::query::queries::SakKey;
    use lib_schemas::skuffen::sak::{Ordningsverdi, Sakstittel};
    use lib_schemas::skuffen::tilgang::Tilgjengelighet;
    use serde_json::{Value, json};
    use uuid::Uuid;

    fn fixed_uuid(suffix: u16) -> Uuid {
        Uuid::parse_str(&format!("123e4567-e89b-12d3-a456-42661417{suffix:04}"))
            .expect("valid fixed uuid")
    }

    fn envelope(command_id: Uuid, payload: Command) -> CommandEnvelope<Command> {
        CommandEnvelope {
            command_id,
            correlation_id: Some(fixed_uuid(900)),
            payload,
        }
    }

    fn sak_key() -> SakKey {
        SakKey::ClientReference(fixed_uuid(1))
    }

    fn dokument() -> Dokument {
        Dokument {
            client_reference: fixed_uuid(10),
            tittel: "Vedlegg".to_string(),
            form: Dokumentform::Bytes {
                dokument_referanse: fixed_uuid(11),
                filtype: "PDF".to_string(),
            },
        }
    }

    fn journalpost_common(client_reference: Uuid) -> JournalpostCommon {
        JournalpostCommon {
            client_reference,
            tittel: "Journalpost".to_string(),
            dokument_dato: "2026-01-01".to_string(),
            saksbehandler: "Z12345".to_string(),
            saksbehandler_enhet: "42".to_string(),
            tilgjengelighet: Tilgjengelighet::Offentlig,
            dokumenter: vec![dokument()],
            sak_key: sak_key(),
            kildesystem: None,
        }
    }

    fn assert_persisted_payload_json(envelope: CommandEnvelope<Command>, expected: Value) {
        let persisted_payload = serde_json::to_value(&envelope).unwrap();
        assert_eq!(persisted_payload, expected);

        let decoded: CommandEnvelope<Command> = serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), expected);
    }

    #[test]
    fn command_execution_payload_json_is_pinned_for_all_command_variants() {
        assert_persisted_payload_json(
            envelope(
                fixed_uuid(100),
                Command::OpprettSak(OpprettSak {
                    client_reference: fixed_uuid(1),
                    sakstittel: Sakstittel::try_from("Test sak".to_string()).unwrap(),
                    arkivdel: Arkivdel::Tilsynsdivisjonene,
                    saksbehandler_id: "Z12345".to_string(),
                    saksbehandler_enhet: "42".to_string(),
                    ordningsverdi: Ordningsverdi::new("123".to_string()).unwrap(),
                    tilgjengelighet: Tilgjengelighet::Offentlig,
                }),
            ),
            json!({
                "command_id": "123e4567-e89b-12d3-a456-426614170100",
                "correlation_id": "123e4567-e89b-12d3-a456-426614170900",
                "payload": {
                    "OpprettSak": {
                        "client_reference": "123e4567-e89b-12d3-a456-426614170001",
                        "sakstittel": "Test sak",
                        "arkivdel": "Tilsynsdivisjonene",
                        "saksbehandler_id": "Z12345",
                        "saksbehandler_enhet": "42",
                        "ordningsverdi": "123",
                        "tilgjengelighet": "Offentlig"
                    }
                }
            }),
        );

        assert_persisted_payload_json(
            envelope(
                fixed_uuid(101),
                Command::AvsluttSak(AvsluttSak { sak_key: sak_key() }),
            ),
            json!({
                "command_id": "123e4567-e89b-12d3-a456-426614170101",
                "correlation_id": "123e4567-e89b-12d3-a456-426614170900",
                "payload": {
                    "AvsluttSak": {
                        "sak_key": {
                            "type": "clientReference",
                            "value": "123e4567-e89b-12d3-a456-426614170001"
                        }
                    }
                }
            }),
        );

        assert_persisted_payload_json(
            envelope(
                fixed_uuid(102),
                Command::SettSaksansvarlig(SettSaksansvarlig {
                    sak_key: sak_key(),
                    saksbehandler_id: "Z12345".to_string(),
                    saksbehandler_enhet: "42".to_string(),
                }),
            ),
            json!({
                "command_id": "123e4567-e89b-12d3-a456-426614170102",
                "correlation_id": "123e4567-e89b-12d3-a456-426614170900",
                "payload": {
                    "SettSaksansvarlig": {
                        "sak_key": {
                            "type": "clientReference",
                            "value": "123e4567-e89b-12d3-a456-426614170001"
                        },
                        "saksbehandler_id": "Z12345",
                        "saksbehandler_enhet": "42"
                    }
                }
            }),
        );

        assert_persisted_payload_json(
            envelope(
                fixed_uuid(200),
                Command::OpprettInngåendeJournalpost(OpprettInngåendeJournalpost {
                    felles: journalpost_common(fixed_uuid(2)),
                    avsender: Korrespondansepart {
                        navn: "Avsender".to_string(),
                        parttype: Parttype::Virksomhet,
                    },
                }),
            ),
            json!({
                "command_id": "123e4567-e89b-12d3-a456-426614170200",
                "correlation_id": "123e4567-e89b-12d3-a456-426614170900",
                "payload": {
                    "OpprettInngåendeJournalpost": {
                        "client_reference": "123e4567-e89b-12d3-a456-426614170002",
                        "tittel": "Journalpost",
                        "dokument_dato": "2026-01-01",
                        "saksbehandler": "Z12345",
                        "saksbehandler_enhet": "42",
                        "dokumenter": [{
                            "client_reference": "123e4567-e89b-12d3-a456-426614170010",
                            "tittel": "Vedlegg",
                            "form": {
                                "Bytes": {
                                    "dokument_referanse": "123e4567-e89b-12d3-a456-426614170011",
                                    "filtype": "PDF"
                                }
                            }
                        }],
                        "tilgjengelighet": "Offentlig",
                        "sak_key": {
                            "type": "clientReference",
                            "value": "123e4567-e89b-12d3-a456-426614170001"
                        },
                        "avsender": { "navn": "Avsender", "parttype": "Virksomhet" }
                    }
                }
            }),
        );

        assert_persisted_payload_json(
            envelope(
                fixed_uuid(201),
                Command::OpprettUtgåendeJournalpost(OpprettUtgåendeJournalpost {
                    felles: journalpost_common(fixed_uuid(3)),
                    mottakere: vec![Korrespondansepart {
                        navn: "Mottaker".to_string(),
                        parttype: Parttype::Virksomhet,
                    }],
                }),
            ),
            json!({
                "command_id": "123e4567-e89b-12d3-a456-426614170201",
                "correlation_id": "123e4567-e89b-12d3-a456-426614170900",
                "payload": {
                    "OpprettUtgåendeJournalpost": {
                        "client_reference": "123e4567-e89b-12d3-a456-426614170003",
                        "tittel": "Journalpost",
                        "dokument_dato": "2026-01-01",
                        "saksbehandler": "Z12345",
                        "saksbehandler_enhet": "42",
                        "dokumenter": [{
                            "client_reference": "123e4567-e89b-12d3-a456-426614170010",
                            "tittel": "Vedlegg",
                            "form": {
                                "Bytes": {
                                    "dokument_referanse": "123e4567-e89b-12d3-a456-426614170011",
                                    "filtype": "PDF"
                                }
                            }
                        }],
                        "tilgjengelighet": "Offentlig",
                        "sak_key": {
                            "type": "clientReference",
                            "value": "123e4567-e89b-12d3-a456-426614170001"
                        },
                        "mottakere": [{ "navn": "Mottaker", "parttype": "Virksomhet" }]
                    }
                }
            }),
        );

        assert_persisted_payload_json(
            envelope(
                fixed_uuid(202),
                Command::OpprettInterntNotatJournalpost(OpprettInterntNotatJournalpost {
                    felles: journalpost_common(fixed_uuid(4)),
                }),
            ),
            json!({
                "command_id": "123e4567-e89b-12d3-a456-426614170202",
                "correlation_id": "123e4567-e89b-12d3-a456-426614170900",
                "payload": {
                    "OpprettInterntNotatJournalpost": {
                        "client_reference": "123e4567-e89b-12d3-a456-426614170004",
                        "tittel": "Journalpost",
                        "dokument_dato": "2026-01-01",
                        "saksbehandler": "Z12345",
                        "saksbehandler_enhet": "42",
                        "dokumenter": [{
                            "client_reference": "123e4567-e89b-12d3-a456-426614170010",
                            "tittel": "Vedlegg",
                            "form": {
                                "Bytes": {
                                    "dokument_referanse": "123e4567-e89b-12d3-a456-426614170011",
                                    "filtype": "PDF"
                                }
                            }
                        }],
                        "tilgjengelighet": "Offentlig",
                        "sak_key": {
                            "type": "clientReference",
                            "value": "123e4567-e89b-12d3-a456-426614170001"
                        }
                    }
                }
            }),
        );
    }

    #[test]
    fn klassifiserer_alle_domain_violation_details() {
        let cases: &[(DomainViolation, &str)] = &[
            (JournalpostMangler, "journalpost_mangler"),
            (DokumentFeiletPermanent, "dokument_feilet_permanent"),
            (JournalpostFeiletPermanent, "journalpost_feilet_permanent"),
            (SakFeiletPermanent, "sak_feilet_permanent"),
            (JournalpostTypeMismatch, "journalpost_type_mismatch"),
        ];

        for (violation, expected) in cases {
            assert_eq!(
                error_category_for_detail(violation.safe_detail()),
                *expected
            );
        }
    }
}
