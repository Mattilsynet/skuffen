use application::command::ports::command_execution_port::{
    CommandExecutionRepository, EksekveringKommando, EksekveringsregistreringResultat,
    NyKommandoEksekvering,
};
use application::command::ports::execution_registration_port::EksekveringssystemRegistration;
use application::command::ports::execution_snapshot_port::{
    DokumentState, EksekveringSnapshotRepository, JournalpostOpprettetTransition,
    JournalpostOvergangVedJournalfoering, JournalpostState, SakState, SakStatus, SakTransition,
};
use async_trait::async_trait;
use domain::eksekvering::execution::Ventegrunn;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::plan::JournalpostType;
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

    async fn insert_sak_state_if_absent(
        &self,
        sak_id: Uuid,
        state: &SakState,
    ) -> Result<bool, anyhow::Error> {
        let status = match state.status {
            SakStatus::UnderBehandling => "B",
            SakStatus::Ferdig => "F",
            SakStatus::Avsluttet => "A",
        };
        let result = sqlx::query(
            r#"
            INSERT INTO sak_state (sak_id, status, opprettet, saksnummer)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (sak_id)
            DO NOTHING
            "#,
        )
        .bind(sak_id)
        .bind(status)
        .bind(state.opprettet)
        .bind(state.saksnummer.clone())
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn upsert_sak_state(&self, sak_id: Uuid, state: &SakState) -> Result<(), anyhow::Error> {
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
        .bind(state.saksnummer.clone())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn insert_journalpost_state_if_absent(
        &self,
        journalpost_id: Uuid,
        sak_id: Uuid,
        state: &JournalpostState,
    ) -> Result<bool, anyhow::Error> {
        let result = sqlx::query(
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
            DO NOTHING
            "#,
        )
        .bind(journalpost_id)
        .bind(sak_id)
        .bind(state.journalfoert)
        .bind(state.avskrevet)
        .bind(state.ekspedert)
        .bind(state.har_feilede_dokumenter)
        .bind(state.med_utsending)
        .bind(journalposttype_code(state.journalposttype))
        .bind(state.journalpostnummer)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn insert_dokument_state_if_absent(
        &self,
        dokument_id: Uuid,
        journalpost_id: Uuid,
        state: &DokumentState,
    ) -> Result<bool, anyhow::Error> {
        let result = sqlx::query(
            r#"
            INSERT INTO dokument_state (dokument_id, journalpost_id, lagt_til, irrecoverable_feil)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (dokument_id)
            DO NOTHING
            "#,
        )
        .bind(dokument_id)
        .bind(journalpost_id)
        .bind(state.lagt_til)
        .bind(state.irrecoverable_feil)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

#[async_trait]
impl EksekveringSnapshotRepository for PostgresExecutionStore {
    async fn hent_sak_state(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Option<SakState>, anyhow::Error> {
        let row: Option<(String, bool, Option<String>)> = sqlx::query_as(
            r#"
            SELECT status, opprettet, saksnummer
            FROM sak_state
            WHERE sak_id = $1
            "#,
        )
        .bind(Uuid::from(sak_id))
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

    async fn ensure_sak_state(
        &self,
        sak_id: SkuffenSakId,
        state: SakState,
    ) -> Result<SakState, anyhow::Error> {
        let sak_id = Uuid::from(sak_id);
        if self.insert_sak_state_if_absent(sak_id, &state).await? {
            return Ok(state);
        }

        <Self as EksekveringSnapshotRepository>::hent_sak_state(self, SkuffenSakId::from(sak_id))
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("Fant ikke sak_state etter ensure for sak_id {}", sak_id)
            })
    }

    async fn anvend_sak_transition(
        &self,
        sak_id: SkuffenSakId,
        transition: SakTransition,
    ) -> Result<SakState, anyhow::Error> {
        let sak_id = Uuid::from(sak_id);
        let next_state = SakState {
            status: transition.status,
            opprettet: transition.opprettet,
            saksnummer: transition.saksnummer,
        };
        self.upsert_sak_state(sak_id, &next_state).await?;
        Ok(next_state)
    }

    async fn hent_journalpost_state(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Option<JournalpostState>, anyhow::Error> {
        let row: Option<(bool, bool, bool, bool, bool, String, Option<i32>)> = sqlx::query_as(
            r#"
            SELECT journalfoert, avskrevet, ekspedert, har_feilede_dokumenter, med_utsending, journalposttype, journalpostnummer
            FROM journalpost_state
            WHERE journalpost_id = $1
            "#,
        )
        .bind(Uuid::from(journalpost_id))
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
                journalposttype: parse_journalposttype(&journalposttype),
                journalpostnummer,
            },
        ))
    }

    async fn ensure_journalpost_state(
        &self,
        journalpost_id: SkuffenJournalpostId,
        sak_id: SkuffenSakId,
        state: JournalpostState,
    ) -> Result<JournalpostState, anyhow::Error> {
        let journalpost_id_uuid = Uuid::from(journalpost_id);
        let sak_id_uuid = Uuid::from(sak_id);
        if self
            .insert_journalpost_state_if_absent(journalpost_id_uuid, sak_id_uuid, &state)
            .await?
        {
            return Ok(state);
        }

        <Self as EksekveringSnapshotRepository>::hent_journalpost_state(self, journalpost_id)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Fant ikke journalpost_state etter ensure for journalpost_id {}",
                    journalpost_id_uuid
                )
            })
    }

    async fn anvend_journalpost_opprettet(
        &self,
        journalpost_id: SkuffenJournalpostId,
        transition: JournalpostOpprettetTransition,
    ) -> Result<JournalpostState, anyhow::Error> {
        let journalpost_id = Uuid::from(journalpost_id);
        let result = sqlx::query(
            r#"
            UPDATE journalpost_state
            SET journalpostnummer = COALESCE(journalpostnummer, $2),
                updated_at = now()
            WHERE journalpost_id = $1
            "#,
        )
        .bind(journalpost_id)
        .bind(transition.journalpostnummer)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Fant ikke journalpost_state for journalpost_id {}",
                journalpost_id
            ));
        }

        <Self as EksekveringSnapshotRepository>::hent_journalpost_state(
            self,
            SkuffenJournalpostId::from(journalpost_id),
        )
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Fant ikke journalpost_state etter opprettet for journalpost_id {}",
                journalpost_id
            )
        })
    }

    async fn anvend_journalpost_overgang_ved_journalfoering(
        &self,
        journalpost_id: SkuffenJournalpostId,
        transition: JournalpostOvergangVedJournalfoering,
    ) -> Result<JournalpostState, anyhow::Error> {
        let journalpost_id = Uuid::from(journalpost_id);
        let result = sqlx::query(
            r#"
            UPDATE journalpost_state
            SET journalfoert = $2,
                ekspedert = $3,
                updated_at = now()
            WHERE journalpost_id = $1
            "#,
        )
        .bind(journalpost_id)
        .bind(transition.journalfoert)
        .bind(transition.ekspedert)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Fant ikke journalpost_state for journalpost_id {}",
                journalpost_id
            ));
        }

        <Self as EksekveringSnapshotRepository>::hent_journalpost_state(
            self,
            SkuffenJournalpostId::from(journalpost_id),
        )
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Fant ikke journalpost_state etter journalfoering for journalpost_id {}",
                journalpost_id
            )
        })
    }

    async fn anvend_journalpost_avskrevet(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<JournalpostState, anyhow::Error> {
        let journalpost_id = Uuid::from(journalpost_id);
        let result = sqlx::query(
            r#"
            UPDATE journalpost_state
            SET avskrevet = true,
                updated_at = now()
            WHERE journalpost_id = $1
            "#,
        )
        .bind(journalpost_id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Fant ikke journalpost_state for journalpost_id {}",
                journalpost_id
            ));
        }

        <Self as EksekveringSnapshotRepository>::hent_journalpost_state(
            self,
            SkuffenJournalpostId::from(journalpost_id),
        )
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Fant ikke journalpost_state etter avskriving for journalpost_id {}",
                journalpost_id
            )
        })
    }

    async fn hent_journalposter_for_sak(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Vec<JournalpostState>, anyhow::Error> {
        let rows: Vec<(bool, bool, bool, bool, bool, String, Option<i32>)> = sqlx::query_as(
            r#"
            SELECT journalfoert, avskrevet, ekspedert, har_feilede_dokumenter, med_utsending, journalposttype, journalpostnummer
            FROM journalpost_state
            WHERE sak_id = $1
            "#,
        )
        .bind(Uuid::from(sak_id))
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
                    journalposttype: parse_journalposttype(&journalposttype),
                    journalpostnummer,
                },
            )
            .collect())
    }

    async fn hent_dokument_state(
        &self,
        dokument_id: SkuffenDokumentId,
    ) -> Result<Option<DokumentState>, anyhow::Error> {
        let row: Option<(bool, bool)> = sqlx::query_as(
            r#"
            SELECT lagt_til, irrecoverable_feil
            FROM dokument_state
            WHERE dokument_id = $1
            "#,
        )
        .bind(Uuid::from(dokument_id))
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(lagt_til, irrecoverable_feil)| DokumentState {
            lagt_til,
            irrecoverable_feil,
        }))
    }

    async fn ensure_dokument_state(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
        state: DokumentState,
    ) -> Result<DokumentState, anyhow::Error> {
        let dokument_id_uuid = Uuid::from(dokument_id);
        let journalpost_id_uuid = Uuid::from(journalpost_id);
        if self
            .insert_dokument_state_if_absent(dokument_id_uuid, journalpost_id_uuid, &state)
            .await?
        {
            return Ok(state);
        }

        <Self as EksekveringSnapshotRepository>::hent_dokument_state(self, dokument_id)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Fant ikke dokument_state etter ensure for dokument_id {}",
                    dokument_id_uuid
                )
            })
    }

    async fn anvend_dokument_lagt_til(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<DokumentState, anyhow::Error> {
        let dokument_id = Uuid::from(dokument_id);
        let journalpost_id = Uuid::from(journalpost_id);
        let result = sqlx::query(
            r#"
            UPDATE dokument_state
            SET lagt_til = true,
                updated_at = now()
            WHERE dokument_id = $1 AND journalpost_id = $2
            "#,
        )
        .bind(dokument_id)
        .bind(journalpost_id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Fant ikke dokument_state for dokument_id {} journalpost_id {}",
                dokument_id,
                journalpost_id
            ));
        }

        <Self as EksekveringSnapshotRepository>::hent_dokument_state(
            self,
            SkuffenDokumentId::from(dokument_id),
        )
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Fant ikke dokument_state etter lagt_til for dokument_id {}",
                dokument_id
            )
        })
    }

    async fn anvend_dokument_irrecoverable_feil(
        &self,
        dokument_id: SkuffenDokumentId,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<DokumentState, anyhow::Error> {
        let mut tx = self.pool.begin().await?;
        let dokument_id = Uuid::from(dokument_id);
        let journalpost_id = Uuid::from(journalpost_id);

        let result = sqlx::query(
            r#"
            UPDATE dokument_state
            SET irrecoverable_feil = true,
                lagt_til = false,
                updated_at = now()
            WHERE dokument_id = $1 AND journalpost_id = $2
            "#,
        )
        .bind(dokument_id)
        .bind(journalpost_id)
        .execute(&mut *tx)
        .await?;

        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Fant ikke dokument_state for dokument_id {} journalpost_id {}",
                dokument_id,
                journalpost_id
            ));
        }

        sqlx::query(
            r#"
            UPDATE journalpost_state
            SET har_feilede_dokumenter = true,
                updated_at = now()
            WHERE journalpost_id = $1
            "#,
        )
        .bind(journalpost_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        <Self as EksekveringSnapshotRepository>::hent_dokument_state(
            self,
            SkuffenDokumentId::from(dokument_id),
        )
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Fant ikke dokument_state etter irrecoverable_feil for dokument_id {}",
                dokument_id
            )
        })
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
        registration: &EksekveringssystemRegistration,
        ny: NyKommandoEksekvering,
    ) -> Result<EksekveringsregistreringResultat, anyhow::Error> {
        let mut tx = self.pool.begin().await?;

        if let Some(sak) = &registration.sak {
            sqlx::query(
                r#"
                INSERT INTO sak_state (sak_id, status, opprettet, saksnummer)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (sak_id) DO NOTHING
                "#,
            )
            .bind(Uuid::from(sak.sak_id))
            .bind(sak_status_code(sak.state.status))
            .bind(sak.state.opprettet)
            .bind(sak.state.saksnummer.clone())
            .execute(&mut *tx)
            .await?;
        }

        if let Some(journalpost) = &registration.journalpost {
            sqlx::query(
                r#"
                INSERT INTO journalpost_state (
                    journalpost_id, sak_id, journalfoert, avskrevet, ekspedert,
                    har_feilede_dokumenter, med_utsending, journalposttype, journalpostnummer
                )
                VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
                ON CONFLICT (journalpost_id) DO NOTHING
                "#,
            )
            .bind(Uuid::from(journalpost.journalpost_id))
            .bind(Uuid::from(journalpost.sak_id))
            .bind(journalpost.state.journalfoert)
            .bind(journalpost.state.avskrevet)
            .bind(journalpost.state.ekspedert)
            .bind(journalpost.state.har_feilede_dokumenter)
            .bind(journalpost.state.med_utsending)
            .bind(journalposttype_code(journalpost.state.journalposttype))
            .bind(journalpost.state.journalpostnummer)
            .execute(&mut *tx)
            .await?;
        }

        for dokument in &registration.dokumenter {
            sqlx::query(
                r#"
                INSERT INTO dokument_state (dokument_id, journalpost_id, lagt_til, irrecoverable_feil)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (dokument_id) DO NOTHING
                "#,
            )
            .bind(Uuid::from(dokument.dokument_id))
            .bind(Uuid::from(dokument.journalpost_id))
            .bind(dokument.state.lagt_til)
            .bind(dokument.state.irrecoverable_feil)
            .execute(&mut *tx)
            .await?;
        }

        let payload = serde_json::to_value(&ny.envelope)?;
        let result = sqlx::query(
            r#"
            INSERT INTO command_execution (
                command_id, correlation_id, payload, command_type, sak_id, journalpost_id,
                status, attempt_no, retry_ready_at, wait_kind, wait_sak_id, wait_journalpost_id,
                last_detail, utfores_venter_publisert_at, finished_at
            )
            VALUES ($1,$2,$3,$4,$5,$6,$7,0,NULL,$8,$9,$10,$11,NULL,$12)
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
        .bind(ny.ventegrunn.as_ref().map(Ventegrunn::kind_code))
        .bind(
            ny.ventegrunn
                .as_ref()
                .and_then(|grunn| grunn.sak_id())
                .map(Uuid::from),
        )
        .bind(
            ny.ventegrunn
                .as_ref()
                .and_then(|grunn| grunn.journalpost_id())
                .map(Uuid::from),
        )
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
                wait_kind = NULL,
                wait_sak_id = NULL,
                wait_journalpost_id = NULL,
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
                finished_at = NULL,
                wait_kind = NULL,
                wait_sak_id = NULL,
                wait_journalpost_id = NULL
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

    async fn marker_venter(
        &self,
        command_id: Uuid,
        attempt_no: i32,
        grunn: &Ventegrunn,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            r#"
            UPDATE command_execution
            SET status = 'venter',
                retry_ready_at = NULL,
                wait_kind = $3,
                wait_sak_id = $4,
                wait_journalpost_id = $5,
                last_detail = $6,
                updated_at = now(),
                finished_at = NULL
            WHERE command_id = $1
              AND status = 'kjorer'
              AND attempt_no = $2
            "#,
        )
        .bind(command_id)
        .bind(attempt_no)
        .bind(grunn.kind_code())
        .bind(grunn.sak_id().map(Uuid::from))
        .bind(grunn.journalpost_id().map(Uuid::from))
        .bind(detalj)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            return Err(anyhow::anyhow!(
                "Kunne ikke markere command {} som venter for attempt {}",
                command_id,
                attempt_no
            ));
        }
        avslutt_forsok(&mut tx, command_id, attempt_no, "venter", Some(detalj)).await?;
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
                wait_kind = NULL,
                wait_sak_id = NULL,
                wait_journalpost_id = NULL,
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

    async fn hent_ventende_for_sak(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error> {
        hent_ventende(&self.pool, Some(Uuid::from(sak_id)), None).await
    }

    async fn hent_ventende_for_journalpost(
        &self,
        journalpost_id: SkuffenJournalpostId,
    ) -> Result<Vec<EksekveringKommando>, anyhow::Error> {
        hent_ventende(&self.pool, None, Some(Uuid::from(journalpost_id))).await
    }

    async fn oppdater_til_klar(&self, command_id: Uuid) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE command_execution
            SET status = 'klar',
                retry_ready_at = NULL,
                wait_kind = NULL,
                wait_sak_id = NULL,
                wait_journalpost_id = NULL,
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

    async fn oppdater_venter(
        &self,
        command_id: Uuid,
        grunn: &Ventegrunn,
        detalj: &str,
    ) -> Result<(), anyhow::Error> {
        sqlx::query(
            r#"
            UPDATE command_execution
            SET status = 'venter',
                retry_ready_at = NULL,
                wait_kind = $2,
                wait_sak_id = $3,
                wait_journalpost_id = $4,
                last_detail = $5,
                updated_at = now(),
                finished_at = NULL
            WHERE command_id = $1
            "#,
        )
        .bind(command_id)
        .bind(grunn.kind_code())
        .bind(grunn.sak_id().map(Uuid::from))
        .bind(grunn.journalpost_id().map(Uuid::from))
        .bind(detalj)
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
                wait_kind = NULL,
                wait_sak_id = NULL,
                wait_journalpost_id = NULL,
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
                wait_kind = NULL,
                wait_sak_id = NULL,
                wait_journalpost_id = NULL,
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

fn parse_journalposttype(journalposttype: &str) -> JournalpostType {
    match journalposttype {
        "I" => JournalpostType::Inngaende,
        "U" => JournalpostType::Utgaaende,
        "X" => JournalpostType::InterntNotat,
        _ => JournalpostType::Inngaende,
    }
}

fn sak_status_code(status: SakStatus) -> &'static str {
    match status {
        SakStatus::UnderBehandling => "B",
        SakStatus::Ferdig => "F",
        SakStatus::Avsluttet => "A",
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

async fn hent_ventende(
    pool: &PgPool,
    sak_id: Option<Uuid>,
    journalpost_id: Option<Uuid>,
) -> Result<Vec<EksekveringKommando>, anyhow::Error> {
    let rows: Vec<(Uuid, serde_json::Value, i32, bool)> = sqlx::query_as(
        r#"
        SELECT command_id, payload, attempt_no, utfores_venter_publisert_at IS NOT NULL
        FROM command_execution
        WHERE status = 'venter'
          AND (($1::uuid IS NOT NULL AND wait_sak_id = $1) OR ($2::uuid IS NOT NULL AND wait_journalpost_id = $2))
        ORDER BY created_at
        "#,
    )
    .bind(sak_id)
    .bind(journalpost_id)
    .fetch_all(pool)
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

fn journalposttype_code(journalposttype: JournalpostType) -> &'static str {
    match journalposttype {
        JournalpostType::Inngaende => "I",
        JournalpostType::Utgaaende => "U",
        JournalpostType::InterntNotat => "X",
    }
}
