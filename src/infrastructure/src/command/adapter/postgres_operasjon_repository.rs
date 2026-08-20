use anyhow::{Context, Result};
use application::command::materialisering::Dekomponeringsplan;
use application::command::ports::operasjon_port::{
    CommandMetadata, CommandOutcome, Dekomponeringsresultat, Faktaoppdatering, Gjenoppretting,
    OperasjonRepository,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::operasjon::{
    EntitetId, Operasjon, OperasjonId, OperasjonSammendrag, Operasjonsstatus, Operasjonstype,
};
use domain::eksekvering::tilstand::JournalpostTilstand;
use domain::eksekvering::typer::{CommandTypeCode, Statuskontekst};
use lib_sql::database_config::DbPool;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub struct PostgresOperasjonRepository {
    pool: DbPool,
}

impl PostgresOperasjonRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

fn tilstand_kode(tilstand: JournalpostTilstand) -> &'static str {
    match tilstand {
        JournalpostTilstand::IkkeOpprettet => "ikke_opprettet",
        JournalpostTilstand::Opprettet => "opprettet",
        JournalpostTilstand::KlarForEkspedering => "klar_for_ekspedering",
        JournalpostTilstand::Ekspedert => "ekspedert",
        JournalpostTilstand::Journalfoert => "journalfoert",
        JournalpostTilstand::Avskrevet => "avskrevet",
    }
}

/// Skriver faktaendringen operasjonen medførte.
///
/// Kjøres alltid inne i samme transaksjon som statusovergangen (SKU-0016 R4).
/// Feiler den, rulles statusovergangen tilbake, og operasjonen blir stående i
/// `sendt` — som ved recovery blir `krever_avklaring` i stedet for et duplikat
/// i arkivet.
async fn skriv_fakta(
    tx: &mut Transaction<'_, Postgres>,
    sak_id: SkuffenSakId,
    oppdatering: Faktaoppdatering,
) -> Result<()> {
    match oppdatering {
        Faktaoppdatering::Ingen => {}
        Faktaoppdatering::SakOpprettet { arkiv_id } => {
            let sak_uuid: Uuid = sak_id.into();
            sqlx::query("UPDATE entitet SET arkiv_id = $2 WHERE skuffen_id = $1")
                .bind(sak_uuid)
                .bind(&arkiv_id)
                .execute(&mut **tx)
                .await
                .context("failed to record sak arkiv id")?;
            sqlx::query("UPDATE sak_tilstand SET tilstand = 'opprettet' WHERE sak_id = $1")
                .bind(sak_uuid)
                .execute(&mut **tx)
                .await
                .context("failed to record sak opprettet")?;
        }
        Faktaoppdatering::SakAvsluttet => {
            sqlx::query("UPDATE sak_tilstand SET tilstand = 'avsluttet' WHERE sak_id = $1")
                .bind(Uuid::from(sak_id))
                .execute(&mut **tx)
                .await
                .context("failed to record sak avsluttet")?;
        }
        Faktaoppdatering::SaksansvarligSatt {
            saksbehandler_id,
            saksbehandler_enhet,
        } => {
            sqlx::query(
                r#"UPDATE sak_tilstand
                   SET naavaerende_saksansvarlig_id = $2,
                       naavaerende_saksansvarlig_enhet = $3
                   WHERE sak_id = $1"#,
            )
            .bind(Uuid::from(sak_id))
            .bind(&saksbehandler_id)
            .bind(&saksbehandler_enhet)
            .execute(&mut **tx)
            .await
            .context("failed to record saksansvarlig")?;
        }
        Faktaoppdatering::DokumentRendret {
            dokument_id,
            rendered_dokument_referanse,
        } => {
            sqlx::query(
                r#"UPDATE dokument_tilstand
                   SET rendered_dokument_referanse = $2, tilstand = 'klar'
                   WHERE dokument_id = $1"#,
            )
            .bind(Uuid::from(dokument_id))
            .bind(rendered_dokument_referanse)
            .execute(&mut **tx)
            .await
            .context("failed to record rendered dokument")?;
        }
        Faktaoppdatering::JournalpostOpprettet {
            journalpost_id,
            arkiv_id,
            hoveddokument_id,
        } => {
            sqlx::query("UPDATE entitet SET arkiv_id = $2 WHERE skuffen_id = $1")
                .bind(Uuid::from(journalpost_id))
                .bind(&arkiv_id)
                .execute(&mut **tx)
                .await
                .context("failed to record journalpost arkiv id")?;
            sqlx::query(
                "UPDATE journalpost_tilstand SET tilstand = 'opprettet' WHERE journalpost_id = $1",
            )
            .bind(Uuid::from(journalpost_id))
            .execute(&mut **tx)
            .await
            .context("failed to record journalpost opprettet")?;
            // Hoveddokumentet følger med opprettelsen og ligger dermed i arkivet.
            sqlx::query("UPDATE dokument_tilstand SET tilstand = 'ok' WHERE dokument_id = $1")
                .bind(Uuid::from(hoveddokument_id))
                .execute(&mut **tx)
                .await
                .context("failed to record hoveddokument arkivert")?;
        }
        Faktaoppdatering::VedleggArkivert {
            dokument_id,
            arkiv_id,
        } => {
            if let Some(arkiv_id) = arkiv_id.as_deref() {
                sqlx::query("UPDATE entitet SET arkiv_id = $2 WHERE skuffen_id = $1")
                    .bind(Uuid::from(dokument_id))
                    .bind(arkiv_id)
                    .execute(&mut **tx)
                    .await
                    .context("failed to record vedlegg arkiv id")?;
            }
            sqlx::query("UPDATE dokument_tilstand SET tilstand = 'ok' WHERE dokument_id = $1")
                .bind(Uuid::from(dokument_id))
                .execute(&mut **tx)
                .await
                .context("failed to record vedlegg arkivert")?;
        }
        Faktaoppdatering::JournalpostStatus {
            journalpost_id,
            tilstand,
        } => {
            sqlx::query("UPDATE journalpost_tilstand SET tilstand = $2 WHERE journalpost_id = $1")
                .bind(Uuid::from(journalpost_id))
                .bind(tilstand_kode(tilstand))
                .execute(&mut **tx)
                .await
                .context("failed to record journalpost status")?;
        }
    }
    Ok(())
}

#[async_trait]
impl OperasjonRepository for PostgresOperasjonRepository {
    async fn try_acquire_executor_lock(&self, _executor_id: &str) -> Result<bool> {
        // Én aktiv executor, håndhevet med advisory lock på en fast nøkkel.
        sqlx::query_scalar("SELECT pg_try_advisory_lock(4711)")
            .fetch_one(&self.pool)
            .await
            .context("failed to acquire executor lock")
    }

    async fn lagre_dekomponering(
        &self,
        plan: Dekomponeringsplan,
    ) -> Result<Dekomponeringsresultat> {
        let mut tx = self.pool.begin().await.context("failed to begin")?;

        super::postgres_dekomponering::skriv_plan(&mut tx, &plan).await?;

        let mut nye = 0u64;
        for operasjon in &plan.operasjoner {
            let resultat = sqlx::query(
                r#"
                INSERT INTO operasjon (operasjon_id, command_id, operasjonstype, entitet_id, sak_id)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (command_id, operasjonstype, entitet_id) DO NOTHING
                "#,
            )
            .bind(Uuid::from(operasjon.operasjon_id))
            .bind(plan.command_id)
            .bind(operasjon.operasjonstype.as_code())
            .bind(operasjon.entitet_id.as_uuid())
            .bind(Uuid::from(operasjon.sak_id))
            .execute(&mut *tx)
            .await
            .context("failed to insert operasjon")?;
            nye += resultat.rows_affected();
        }

        sqlx::query("UPDATE command SET dekomponert_at = now() WHERE command_id = $1")
            .bind(plan.command_id)
            .execute(&mut *tx)
            .await
            .context("failed to mark decomposed")?;

        tx.commit()
            .await
            .context("failed to commit decomposition")?;

        Ok(Dekomponeringsresultat {
            nye_operasjoner: nye,
        })
    }

    async fn hent_neste_kjorbare(&self) -> Result<Option<Operasjon>> {
        let rad: Option<(Uuid, String, Uuid, Uuid, String)> = sqlx::query_as(
            r#"
            SELECT o.operasjon_id, o.operasjonstype, o.entitet_id, o.sak_id, e.entitet_type::text
            FROM operasjon o
            JOIN entitet e ON e.skuffen_id = o.entitet_id
            WHERE o.status = 'klar'
               OR (o.status = 'retry_venter' AND o.neste_forsok_at <= now())
            ORDER BY o.created_at
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .context("failed to fetch runnable operasjon")?;

        rad.map(les_operasjon).transpose()
    }

    async fn marker_kjorer(&self, operasjon_id: OperasjonId, executor_id: &str) -> Result<i32> {
        let mut tx = self.pool.begin().await?;
        let attempt_no: i32 = sqlx::query_scalar(
            r#"UPDATE operasjon
               SET status = 'kjorer', attempt_no = attempt_no + 1, neste_forsok_at = NULL
               WHERE operasjon_id = $1
               RETURNING attempt_no"#,
        )
        .bind(Uuid::from(operasjon_id))
        .fetch_one(&mut *tx)
        .await
        .context("failed to mark operasjon running")?;

        sqlx::query(
            r#"INSERT INTO operasjon_forsok (operasjon_id, attempt_no, executor_id)
               VALUES ($1, $2, $3)
               ON CONFLICT (operasjon_id, attempt_no) DO NOTHING"#,
        )
        .bind(Uuid::from(operasjon_id))
        .bind(attempt_no)
        .bind(executor_id)
        .execute(&mut *tx)
        .await
        .context("failed to open attempt")?;

        tx.commit().await?;
        Ok(attempt_no)
    }

    async fn marker_sendt(&self, operasjon_id: OperasjonId, _attempt_no: i32) -> Result<()> {
        sqlx::query(
            "UPDATE operasjon SET status = 'sendt', sendt_at = now() WHERE operasjon_id = $1",
        )
        .bind(Uuid::from(operasjon_id))
        .execute(&self.pool)
        .await
        .context("failed to mark operasjon sent")?;
        Ok(())
    }

    /// Statusovergang, forsøksutfall og faktaoppdatering i **én** transaksjon.
    ///
    /// Dette er at-most-once-grensen (SKU-0016 R4). Splittes den i flere
    /// commits, kan et vellykket arkivskriv bli usynlig for oss og gi duplikat
    /// ved neste forsøk.
    async fn fullfor_ok(
        &self,
        operasjon_id: OperasjonId,
        attempt_no: i32,
        oppdatering: Faktaoppdatering,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await.context("failed to begin")?;

        let sak_id: Uuid = sqlx::query_scalar(
            r#"UPDATE operasjon
               SET status = 'ok', ferdig_at = now(), siste_detalj = NULL
               WHERE operasjon_id = $1
               RETURNING sak_id"#,
        )
        .bind(Uuid::from(operasjon_id))
        .fetch_one(&mut *tx)
        .await
        .context("failed to mark operasjon ok")?;

        if attempt_no > 0 {
            sqlx::query(
                r#"UPDATE operasjon_forsok
                   SET avsluttet_at = now(), utfall = 'ok'
                   WHERE operasjon_id = $1 AND attempt_no = $2"#,
            )
            .bind(Uuid::from(operasjon_id))
            .bind(attempt_no)
            .execute(&mut *tx)
            .await
            .context("failed to close attempt")?;
        }

        skriv_fakta(&mut tx, SkuffenSakId::from(sak_id), oppdatering).await?;

        tx.commit().await.context("failed to commit operasjon ok")?;
        Ok(())
    }

    async fn fullfor_poll(
        &self,
        operasjon_id: OperasjonId,
        attempt_no: i32,
        oppdatering: Faktaoppdatering,
        neste_forsok_at: DateTime<Utc>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        let sak_id: Uuid = sqlx::query_scalar(
            r#"UPDATE operasjon
               SET status = 'retry_venter', neste_forsok_at = $2, sendt_at = NULL
               WHERE operasjon_id = $1
               RETURNING sak_id"#,
        )
        .bind(Uuid::from(operasjon_id))
        .bind(neste_forsok_at)
        .fetch_one(&mut *tx)
        .await
        .context("failed to schedule next poll")?;

        sqlx::query(
            r#"UPDATE operasjon_forsok
               SET avsluttet_at = now(), utfall = 'retry_venter'
               WHERE operasjon_id = $1 AND attempt_no = $2"#,
        )
        .bind(Uuid::from(operasjon_id))
        .bind(attempt_no)
        .execute(&mut *tx)
        .await?;

        skriv_fakta(&mut tx, SkuffenSakId::from(sak_id), oppdatering).await?;

        tx.commit().await?;
        Ok(())
    }

    async fn marker_retry(
        &self,
        operasjon_id: OperasjonId,
        attempt_no: i32,
        detalj: &str,
        neste_forsok_at: DateTime<Utc>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"UPDATE operasjon
               SET status = 'retry_venter', neste_forsok_at = $2, siste_detalj = $3, sendt_at = NULL
               WHERE operasjon_id = $1"#,
        )
        .bind(Uuid::from(operasjon_id))
        .bind(neste_forsok_at)
        .bind(detalj)
        .execute(&mut *tx)
        .await?;
        lukk_forsok(&mut tx, operasjon_id, attempt_no, "retry_venter", detalj).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn marker_feilet(
        &self,
        operasjon_id: OperasjonId,
        attempt_no: i32,
        detalj: &str,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            r#"UPDATE operasjon
               SET status = 'feilet', ferdig_at = now(), siste_detalj = $2
               WHERE operasjon_id = $1"#,
        )
        .bind(Uuid::from(operasjon_id))
        .bind(detalj)
        .execute(&mut *tx)
        .await?;
        if attempt_no > 0 {
            lukk_forsok(&mut tx, operasjon_id, attempt_no, "feilet", detalj).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn marker_blokkert(
        &self,
        operasjon_id: OperasjonId,
        _attempt_no: Option<i32>,
        detalj: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"UPDATE operasjon
               SET status = 'blokkert', siste_detalj = $2, neste_forsok_at = NULL
               WHERE operasjon_id = $1"#,
        )
        .bind(Uuid::from(operasjon_id))
        .bind(detalj)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn marker_klar(&self, operasjon_id: OperasjonId) -> Result<()> {
        sqlx::query(
            "UPDATE operasjon SET status = 'klar', neste_forsok_at = NULL WHERE operasjon_id = $1",
        )
        .bind(Uuid::from(operasjon_id))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn gjenopprett_etter_restart(&self) -> Result<Gjenoppretting> {
        let mut tx = self.pool.begin().await?;

        // Avbrutt før arkivkallet: trygt å prøve igjen.
        let gjenopptatt =
            sqlx::query("UPDATE operasjon SET status = 'klar' WHERE status = 'kjorer'")
                .execute(&mut *tx)
                .await?
                .rows_affected();

        // Ukjent utfall: et menneske må rydde (SKU-0016 R5).
        let krever_avklaring =
            sqlx::query("UPDATE operasjon SET status = 'krever_avklaring' WHERE status = 'sendt'")
                .execute(&mut *tx)
                .await?
                .rows_affected();

        sqlx::query(
            r#"UPDATE operasjon_forsok
               SET avsluttet_at = now(), utfall = 'avbrutt'
               WHERE avsluttet_at IS NULL"#,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(Gjenoppretting {
            gjenopptatt,
            krever_avklaring,
        })
    }

    async fn hent_blokkerte(&self, grense: i64) -> Result<Vec<Operasjon>> {
        let rader: Vec<(Uuid, String, Uuid, Uuid, String)> = sqlx::query_as(
            r#"
            SELECT o.operasjon_id, o.operasjonstype, o.entitet_id, o.sak_id, e.entitet_type::text
            FROM operasjon o
            JOIN entitet e ON e.skuffen_id = o.entitet_id
            WHERE o.status = 'blokkert'
            ORDER BY o.created_at
            LIMIT $1
            "#,
        )
        .bind(grense)
        .fetch_all(&self.pool)
        .await?;

        rader.into_iter().map(les_operasjon).collect()
    }

    async fn hent_krever_avklaring(&self) -> Result<Vec<Operasjon>> {
        let rader: Vec<(Uuid, String, Uuid, Uuid, String)> = sqlx::query_as(
            r#"
            SELECT o.operasjon_id, o.operasjonstype, o.entitet_id, o.sak_id, e.entitet_type::text
            FROM operasjon o
            JOIN entitet e ON e.skuffen_id = o.entitet_id
            WHERE o.status = 'krever_avklaring'
            ORDER BY o.created_at
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rader.into_iter().map(les_operasjon).collect()
    }

    async fn hent_sammendrag_for_sak(
        &self,
        sak_id: SkuffenSakId,
    ) -> Result<Vec<OperasjonSammendrag>> {
        let rader: Vec<(Uuid, String, String)> = sqlx::query_as(
            r#"SELECT operasjon_id, operasjonstype, status::text
               FROM operasjon WHERE sak_id = $1"#,
        )
        .bind(Uuid::from(sak_id))
        .fetch_all(&self.pool)
        .await?;

        rader
            .into_iter()
            .map(|(id, operasjonstype, status)| {
                Ok(OperasjonSammendrag {
                    operasjon_id: OperasjonId(id),
                    operasjonstype: Operasjonstype::from_code(&operasjonstype)
                        .context("ukjent operasjonstype")?,
                    status: Operasjonsstatus::from_code(&status)
                        .context("ukjent operasjonsstatus")?,
                })
            })
            .collect()
    }

    /// Statuskontekst joines fra `entitet` og state i stedet for å
    /// materialiseres på kommandoen. Én ekstra join per publisert event, mot
    /// en kolonne som ellers kunne divergere fra sannheten.
    async fn hent_command_metadata(&self, operasjon_id: OperasjonId) -> Result<CommandMetadata> {
        let rad: (
            Uuid,
            Option<Uuid>,
            String,
            Option<Uuid>,
            Option<String>,
            Option<Uuid>,
            Option<String>,
            Vec<Uuid>,
        ) = sqlx::query_as(
            r#"
            SELECT
                k.command_id,
                k.correlation_id,
                k.command_type,
                se.client_reference AS sak_client_reference,
                se.arkiv_id         AS saksnummer,
                je.client_reference AS journalpost_client_reference,
                je.arkiv_id         AS journalpost_arkiv_id,
                COALESCE(
                    (SELECT array_agg(de.client_reference ORDER BY dt.rekkefolge)
                       FROM dokument_tilstand dt
                       JOIN entitet de ON de.skuffen_id = dt.dokument_id
                      WHERE dt.opprettet_av_command_id = k.command_id
                        AND de.client_reference IS NOT NULL),
                    ARRAY[]::uuid[]
                ) AS dokument_client_references
            FROM operasjon o
            JOIN command k ON k.command_id = o.command_id
            LEFT JOIN entitet se ON se.skuffen_id = o.sak_id
            LEFT JOIN journalpost_tilstand jt ON jt.opprettet_av_command_id = k.command_id
            LEFT JOIN entitet je ON je.skuffen_id = jt.journalpost_id
            WHERE o.operasjon_id = $1
            "#,
        )
        .bind(Uuid::from(operasjon_id))
        .fetch_one(&self.pool)
        .await
        .context("failed to load command metadata")?;

        let (
            command_id,
            correlation_id,
            command_type,
            sak_client_reference,
            saksnummer,
            journalpost_client_reference,
            journalpost_arkiv_id,
            dokument_client_references,
        ) = rad;

        Ok(CommandMetadata {
            command_id,
            correlation_id,
            command_type: CommandTypeCode::from_code(&command_type)
                .context("ukjent command_type")?,
            kontekst: Statuskontekst {
                sak_client_reference: sak_client_reference.map(|id| id.to_string()),
                saksnummer,
                journalpost_client_reference: journalpost_client_reference.map(|id| id.to_string()),
                journalpost_arkiv_id,
                dokument_client_references: dokument_client_references
                    .into_iter()
                    .map(|id| id.to_string())
                    .collect(),
            },
        })
    }

    async fn hent_status(&self, operasjon_id: OperasjonId) -> Result<Option<Operasjonsstatus>> {
        let status: Option<String> =
            sqlx::query_scalar("SELECT status::text FROM operasjon WHERE operasjon_id = $1")
                .bind(Uuid::from(operasjon_id))
                .fetch_optional(&self.pool)
                .await?;

        status
            .map(|s| Operasjonsstatus::from_code(&s).context("ukjent operasjonsstatus"))
            .transpose()
    }

    /// Foldet over kommandoens operasjoner (SKU-0016 R8). CommandStatus er
    /// ikke en kolonne.
    async fn hent_command_outcome(&self, command_id: Uuid) -> Result<CommandOutcome> {
        let (antall, ok, feilet): (i64, i64, i64) = sqlx::query_as(
            r#"SELECT count(*),
                      count(*) FILTER (WHERE status = 'ok'),
                      count(*) FILTER (WHERE status = 'feilet')
               FROM operasjon WHERE command_id = $1"#,
        )
        .bind(command_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(if feilet > 0 {
            CommandOutcome::Feilet
        } else if antall > 0 && ok == antall {
            CommandOutcome::Fullfort
        } else {
            CommandOutcome::Uavklart
        })
    }

    async fn hent_varselkandidater(&self, eldre_enn: DateTime<Utc>) -> Result<Vec<Operasjon>> {
        let rader: Vec<(Uuid, String, Uuid, Uuid, String)> = sqlx::query_as(
            r#"
            SELECT o.operasjon_id, o.operasjonstype, o.entitet_id, o.sak_id, e.entitet_type::text
            FROM operasjon o
            JOIN entitet e ON e.skuffen_id = o.entitet_id
            WHERE o.varslet_at IS NULL
              AND o.status NOT IN ('ok', 'feilet')
              AND o.created_at < $1
            "#,
        )
        .bind(eldre_enn)
        .fetch_all(&self.pool)
        .await?;

        rader.into_iter().map(les_operasjon).collect()
    }

    async fn marker_varslet(&self, operasjon_id: OperasjonId) -> Result<()> {
        sqlx::query("UPDATE operasjon SET varslet_at = now() WHERE operasjon_id = $1")
            .bind(Uuid::from(operasjon_id))
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

async fn lukk_forsok(
    tx: &mut Transaction<'_, Postgres>,
    operasjon_id: OperasjonId,
    attempt_no: i32,
    utfall: &str,
    detalj: &str,
) -> Result<()> {
    sqlx::query(
        r#"UPDATE operasjon_forsok
           SET avsluttet_at = now(), utfall = $3, detalj = $4
           WHERE operasjon_id = $1 AND attempt_no = $2"#,
    )
    .bind(Uuid::from(operasjon_id))
    .bind(attempt_no)
    .bind(utfall)
    .bind(detalj)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn les_operasjon(rad: (Uuid, String, Uuid, Uuid, String)) -> Result<Operasjon> {
    let (operasjon_id, operasjonstype, entitet_id, sak_id, entitet_type) = rad;
    let entitet_id = match entitet_type.as_str() {
        "sak" => EntitetId::Sak(entitet_id.into()),
        "journalpost" => EntitetId::Journalpost(entitet_id.into()),
        "dokument" => EntitetId::Dokument(entitet_id.into()),
        other => anyhow::bail!("ukjent entitet_type: {other}"),
    };

    Ok(Operasjon {
        operasjon_id: OperasjonId(operasjon_id),
        operasjonstype: Operasjonstype::from_code(&operasjonstype)
            .context("ukjent operasjonstype")?,
        entitet_id,
        sak_id: SkuffenSakId::from(sak_id),
    })
}
