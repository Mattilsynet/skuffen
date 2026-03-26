use application::command::ports::eksekvering_state_port::{
    DokumentState, EksekveringKommando, EksekveringStateRepository, EksekveringStatus,
    EksekveringsregistreringResultat, EksekveringssystemRegistration,
    JournalpostOpprettetTransition, JournalpostOvergangVedJournalfoering, JournalpostState,
    SakState, SakStatus, SakTransition,
};
use async_trait::async_trait;
use domain::eksekvering::id::{SkuffenDokumentId, SkuffenJournalpostId, SkuffenSakId};
use domain::eksekvering::plan::JournalpostType;
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
impl EksekveringStateRepository for PostgresEksekveringStateRepository {
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

        self.hent_sak_state(SkuffenSakId::from(sak_id))
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

        self.hent_journalpost_state(journalpost_id)
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

        self.hent_journalpost_state(SkuffenJournalpostId::from(journalpost_id))
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

        self.hent_journalpost_state(SkuffenJournalpostId::from(journalpost_id))
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

        self.hent_journalpost_state(SkuffenJournalpostId::from(journalpost_id))
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

        self.hent_dokument_state(dokument_id).await?.ok_or_else(|| {
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

        self.hent_dokument_state(SkuffenDokumentId::from(dokument_id))
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Fant ikke dokument_state etter lagt_til for dokument_id {}",
                    dokument_id
                )
            })
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

    async fn ensure_registrert_i_eksekveringssystem(
        &self,
        registration: &EksekveringssystemRegistration,
        envelope: &CommandEnvelope<Command>,
    ) -> Result<EksekveringsregistreringResultat, anyhow::Error> {
        let mut tx = self.pool.begin().await?;

        if let Some(sak) = &registration.sak {
            let status = match sak.state.status {
                SakStatus::UnderBehandling => "B",
                SakStatus::Ferdig => "F",
                SakStatus::Avsluttet => "A",
            };

            sqlx::query(
                r#"
                INSERT INTO sak_state (sak_id, status, opprettet, saksnummer)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (sak_id)
                DO NOTHING
                "#,
            )
            .bind(Uuid::from(sak.sak_id))
            .bind(status)
            .bind(sak.state.opprettet)
            .bind(sak.state.saksnummer.clone())
            .execute(&mut *tx)
            .await?;
        }

        if let Some(journalpost) = &registration.journalpost {
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
                DO NOTHING
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

        let payload = serde_json::to_value(envelope)?;
        let result = sqlx::query(
            r#"
            INSERT INTO command_execution (command_id, correlation_id, payload, status, attempts, utfores_venter_published_at)
            VALUES ($1, $2, $3, 'pending', 0, NULL)
            ON CONFLICT (command_id)
            DO NOTHING
            "#,
        )
        .bind(envelope.command_id)
        .bind(envelope.correlation_id)
        .bind(payload)
        .execute(&mut *tx)
        .await?;

        let registrering = if result.rows_affected() > 0 {
            EksekveringsregistreringResultat::Nyregistrert
        } else {
            let published_at: Option<Option<chrono::DateTime<chrono::Utc>>> = sqlx::query_scalar(
                r#"
                SELECT utfores_venter_published_at
                FROM command_execution
                WHERE command_id = $1
                "#,
            )
            .bind(envelope.command_id)
            .fetch_optional(&mut *tx)
            .await?;

            match published_at {
                Some(Some(_)) => EksekveringsregistreringResultat::EksisterteMedVenterPublisert,
                Some(None) => EksekveringsregistreringResultat::EksisterteUtenVenterPublisert,
                None => {
                    return Err(anyhow::anyhow!(
                        "Fant ikke command_execution etter ensure for command_id {}",
                        envelope.command_id
                    ));
                }
            }
        };

        tx.commit().await?;

        Ok(registrering)
    }

    async fn marker_utfores_venter_publisert(&self, command_id: Uuid) -> Result<(), anyhow::Error> {
        let result = sqlx::query(
            r#"
            UPDATE command_execution
            SET utfores_venter_published_at = COALESCE(utfores_venter_published_at, now()),
                updated_at = now()
            WHERE command_id = $1
            "#,
        )
        .bind(command_id)
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

fn parse_journalposttype(journalposttype: &str) -> JournalpostType {
    match journalposttype {
        "I" => JournalpostType::Inngaende,
        "U" => JournalpostType::Utgaaende,
        "X" => JournalpostType::InterntNotat,
        _ => JournalpostType::Inngaende,
    }
}

fn journalposttype_code(journalposttype: JournalpostType) -> &'static str {
    match journalposttype {
        JournalpostType::Inngaende => "I",
        JournalpostType::Utgaaende => "U",
        JournalpostType::InterntNotat => "X",
    }
}
