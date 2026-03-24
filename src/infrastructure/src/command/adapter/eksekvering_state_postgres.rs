use application::command::ports::eksekvering_state_port::{
    DokumentState, EksekveringKommando, EksekveringStateRepository, EksekveringStatus,
    JournalpostState, SakState, SakStatus,
};
use async_trait::async_trait;
use lib_schemas::skuffen::command::commands::{Command, CommandEnvelope};
use sqlx::postgres::PgPool;
use sqlx::types::chrono;
use uuid::Uuid;

#[derive(Clone)]
pub struct PostgresEksekveringStateRepository {
    pool: PgPool,
}

impl PostgresEksekveringStateRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl EksekveringStateRepository for PostgresEksekveringStateRepository {
    async fn hent_sak_state_fra_state(
        &self,
        sak_id: Uuid,
    ) -> Result<Option<SakState>, anyhow::Error> {
        let row: Option<(String, bool, Option<String>)> = sqlx::query_as(
            r#"
            SELECT status, opprettet, saksnummer
            FROM sak_state
            WHERE sak_id = $1
            "#,
        )
        .bind(sak_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(status, opprettet, saksnummer)| SakState {
            status: match status.as_str() {
                "B" => SakStatus::UnderBehandling,
                "F" => SakStatus::Ferdig,
                "A" => SakStatus::Avsluttet,
                _ => SakStatus::UnderBehandling,
            },
            opprettet,
            saksnummer,
        }))
    }

    async fn lagre_sak_state(&self, sak_id: Uuid, state: SakState) -> Result<(), anyhow::Error> {
        let status = match state.status {
            SakStatus::UnderBehandling => "B",
            SakStatus::Ferdig => "F",
            SakStatus::Avsluttet => "A",
        };
        sqlx::query(
            r#"
            INSERT INTO sak_state (sak_id, status, opprettet, saksnummer)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (sak_id)
            DO UPDATE SET status = $2, opprettet = $3, saksnummer = $4, updated_at = now()
            "#,
        )
        .bind(sak_id)
        .bind(status)
        .bind(state.opprettet)
        .bind(state.saksnummer)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn hent_journalpost_state_fra_state(
        &self,
        journalpost_id: Uuid,
    ) -> Result<Option<JournalpostState>, anyhow::Error> {
        let row: Option<(bool, bool, bool, bool, bool, String, Option<i32>)> = sqlx::query_as(
            r#"
            SELECT journalfoert, avskrevet, ekspedert, har_feilede_dokumenter, med_utsending, journalposttype, journalpostnummer
            FROM journalpost_state
            WHERE journalpost_id = $1
            "#,
        )
        .bind(journalpost_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(
            |(
                journalfoert,
                avskrevet,
                ekspedert,
                har_feilede_dokumenter,
                med_utsending,
                journalposttype,
                journalpostnummer,
            )| JournalpostState {
                journalfoert,
                avskrevet,
                ekspedert,
                har_feilede_dokumenter,
                med_utsending,
                journalposttype: journalposttype.chars().next().unwrap_or('I'),
                journalpostnummer,
            },
        ))
    }

    async fn lagre_journalpost_state(
        &self,
        journalpost_id: Uuid,
        sak_id: Uuid,
        state: JournalpostState,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            INSERT INTO journalpost_state (
                journalpost_id,
                sak_id,
                journalfoert,
                avskrevet,
                ekspedert,
                har_feilede_dokumenter,
                med_utsending,
                journalposttype,
                journalpostnummer
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (journalpost_id)
            DO UPDATE SET
                journalfoert = $3,
                avskrevet = $4,
                ekspedert = $5,
                har_feilede_dokumenter = $6,
                med_utsending = $7,
                journalposttype = $8,
                journalpostnummer = $9,
                updated_at = now()
            "#,
        )
        .bind(journalpost_id)
        .bind(sak_id)
        .bind(state.journalfoert)
        .bind(state.avskrevet)
        .bind(state.ekspedert)
        .bind(state.har_feilede_dokumenter)
        .bind(state.med_utsending)
        .bind(state.journalposttype.to_string())
        .bind(state.journalpostnummer)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn hent_journalposter_for_sak_fra_state(
        &self,
        sak_id: Uuid,
    ) -> Result<Vec<JournalpostState>, anyhow::Error> {
        let rows: Vec<(bool, bool, bool, bool, bool, String, Option<i32>)> = sqlx::query_as(
            r#"
            SELECT journalfoert, avskrevet, ekspedert, har_feilede_dokumenter, med_utsending, journalposttype, journalpostnummer
            FROM journalpost_state
            WHERE sak_id = $1
            "#,
        )
        .bind(sak_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(
                    journalfoert,
                    avskrevet,
                    ekspedert,
                    har_feilede_dokumenter,
                    med_utsending,
                    journalposttype,
                    journalpostnummer,
                )| JournalpostState {
                    journalfoert,
                    avskrevet,
                    ekspedert,
                    har_feilede_dokumenter,
                    med_utsending,
                    journalposttype: journalposttype.chars().next().unwrap_or('I'),
                    journalpostnummer,
                },
            )
            .collect())
    }

    async fn hent_dokument_state_fra_state(
        &self,
        dokument_id: Uuid,
    ) -> Result<Option<DokumentState>, anyhow::Error> {
        let row: Option<(bool, bool)> = sqlx::query_as(
            r#"
            SELECT lagt_til, irrecoverable_feil
            FROM dokument_state
            WHERE dokument_id = $1
            "#,
        )
        .bind(dokument_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(lagt_til, irrecoverable_feil)| DokumentState {
            lagt_til,
            irrecoverable_feil,
        }))
    }

    async fn lagre_dokument_state(
        &self,
        dokument_id: Uuid,
        journalpost_id: Uuid,
        state: DokumentState,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            INSERT INTO dokument_state (dokument_id, journalpost_id, lagt_til, irrecoverable_feil)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (dokument_id)
            DO UPDATE SET lagt_til = $3, irrecoverable_feil = $4, updated_at = now()
            "#,
        )
        .bind(dokument_id)
        .bind(journalpost_id)
        .bind(state.lagt_til)
        .bind(state.irrecoverable_feil)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn oppdater_eksekvering(
        &self,
        command_id: Uuid,
        status: EksekveringStatus,
        last_error: Option<String>,
        next_retry_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), anyhow::Error> {
        let status_str = match status {
            EksekveringStatus::Pending => "pending",
            EksekveringStatus::Running => "running",
            EksekveringStatus::Ok => "ok",
            EksekveringStatus::Blocked => "blocked",
            EksekveringStatus::Error => "error",
            EksekveringStatus::Retrying => "retrying",
        };

        let result = sqlx::query(
            r#"
            UPDATE command_execution
            SET status = $2,
                last_error = $3,
                next_retry_at = $4,
                attempts = attempts + 1,
                locked_at = NULL,
                locked_by = NULL,
                updated_at = now()
            WHERE command_id = $1
            "#,
        )
        .bind(command_id)
        .bind(status_str)
        .bind(last_error)
        .bind(next_retry_at)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Fant ikke command_execution for command_id {}",
                command_id
            ));
        }

        Ok(())
    }

    async fn registrer_kommando(
        &self,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<bool, anyhow::Error> {
        let payload = serde_json::to_value(envelope)?;
        let result = sqlx::query(
            r#"
            INSERT INTO command_execution (command_id, correlation_id, payload, status, attempts)
            VALUES ($1, $2, $3, 'pending', 0)
            ON CONFLICT (command_id)
            DO NOTHING
            "#,
        )
        .bind(envelope.command_id)
        .bind(envelope.correlation_id)
        .bind(payload)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn hent_klare_kommandoer(
        &self,
        limit: i64,
        worker_id: &str,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error> {
        let rows: Vec<(Uuid, serde_json::Value, i32)> = sqlx::query_as(
            r#"
            WITH picked AS (
                SELECT command_id, payload, attempts
                FROM command_execution
                WHERE status IN ('pending', 'retrying', 'blocked')
                  AND (next_retry_at IS NULL OR next_retry_at <= now())
                  AND (locked_at IS NULL OR locked_at < now() - interval '15 minutes')
                ORDER BY created_at
                LIMIT $1
                FOR UPDATE SKIP LOCKED
            )
            UPDATE command_execution ce
            SET status = 'running',
                locked_at = now(),
                locked_by = $2,
                updated_at = now()
            FROM picked
            WHERE ce.command_id = picked.command_id
            RETURNING picked.command_id, picked.payload, picked.attempts
            "#,
        )
        .bind(limit)
        .bind(worker_id)
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for (command_id, payload, attempts) in rows {
            let envelope: CommandEnvelope<Command> = serde_json::from_value(payload)?;
            result.push(EksekveringKommando {
                command_id,
                envelope,
                attempts,
            });
        }

        Ok(result)
    }
}
