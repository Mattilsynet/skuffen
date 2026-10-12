//! Admin retry mot PostgreSQL.
//!
//! Frigivelsen låser sakens operasjoner uten å vente (`NOWAIT`). Admin er
//! dermed aldri den som holder en lås executoren venter på i en syklus, så en
//! vranglås kan ikke gjøre et vellykket arkivskriv til et ukjent utfall.

use std::collections::HashMap;

use anyhow::Context;
use application::admin::model::AdminSakNokkel;
use application::admin::ports::admin_retry_port::{
    AdminRetryRepository, FrigivelseFeil, RetryGrunnlag,
};
use async_trait::async_trait;
use domain::eksekvering::gjenopptak::{
    Operasjonsgjenopptak, sak_sperrer_gjenopptak, vurder_operasjon,
};
use domain::eksekvering::id::SkuffenSakId;
use domain::eksekvering::operasjon::Operasjonsstatus;
use domain::eksekvering::typer::CommandTypeCode;
use lib_sql::database_config::DbPool;
use uuid::Uuid;

use super::postgres_admin_read_repository::hent_sak_identitet;

/// PostgreSQL `lock_not_available`.
const LAAS_IKKE_TILGJENGELIG: &str = "55P03";

pub struct PostgresAdminRetryRepository {
    pool: DbPool,
}

impl PostgresAdminRetryRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AdminRetryRepository for PostgresAdminRetryRepository {
    async fn hent_commands_for_sak(
        &self,
        key: AdminSakNokkel,
        grense: usize,
    ) -> anyhow::Result<Option<Vec<Uuid>>> {
        let grense = i64::try_from(grense).context("admin retry command limit is too large")?;
        let mut tx = self
            .pool
            .begin_with("BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
            .await
            .context("failed to begin admin retry snapshot")?;
        let resultat: anyhow::Result<_> = async {
            let Some(identitet) = hent_sak_identitet(&mut tx, key).await? else {
                return Ok(None);
            };
            let command_ids = sqlx::query_scalar(
                r#"
                SELECT c.command_id
                FROM command c
                WHERE c.sak_id = $1
                   OR (c.sak_id IS NULL AND EXISTS (
                       SELECT 1 FROM operasjon o
                       WHERE o.command_id = c.command_id AND o.sak_id = $1
                   ))
                ORDER BY c.mottatt_at, c.command_id
                LIMIT $2
                "#,
            )
            .bind(identitet.skuffen_id.as_uuid())
            .bind(grense)
            .fetch_all(&mut *tx)
            .await
            .context("failed to load admin retry commands for sak")?;
            Ok(Some(command_ids))
        }
        .await;

        match resultat {
            Ok(verdi) => {
                tx.commit()
                    .await
                    .context("failed to commit admin retry snapshot")?;
                Ok(verdi)
            }
            Err(err) => {
                let _ = tx.rollback().await;
                Err(err)
            }
        }
    }

    async fn hent_grunnlag(&self, command_ids: &[Uuid]) -> anyhow::Result<Vec<RetryGrunnlag>> {
        let rader: Vec<(Uuid, String, Option<Uuid>, bool, Vec<String>)> = sqlx::query_as(
            r#"
            SELECT c.command_id,
                   c.command_type,
                   COALESCE(c.sak_id, (SELECT o.sak_id FROM operasjon o
                                        WHERE o.command_id = c.command_id LIMIT 1)),
                   c.dekomponert_at IS NOT NULL,
                   COALESCE((SELECT array_agg(o.status::text) FROM operasjon o
                              WHERE o.command_id = c.command_id),
                            ARRAY[]::text[])
            FROM command c
            WHERE c.command_id = ANY($1)
            "#,
        )
        .bind(command_ids)
        .fetch_all(&self.pool)
        .await
        .context("failed to load retry basis")?;

        rader
            .into_iter()
            .map(
                |(command_id, command_type, sak_id, dekomponert, statuser)| {
                    Ok(RetryGrunnlag {
                        command_id,
                        command_type: CommandTypeCode::from_code(&command_type)
                            .context("ukjent command_type")?,
                        sak_id: sak_id.map(SkuffenSakId::from),
                        dekomponert,
                        operasjoner: statuser
                            .iter()
                            .map(|status| les_status(status))
                            .collect::<anyhow::Result<_>>()?,
                    })
                },
            )
            .collect()
    }

    async fn frigi(
        &self,
        saker: &[SkuffenSakId],
        command_ids: &[Uuid],
    ) -> Result<(), FrigivelseFeil> {
        let sak_uuids: Vec<Uuid> = saker.iter().map(|sak| Uuid::from(*sak)).collect();
        let mut tx = self.pool.begin().await.context("failed to begin")?;

        let rader: Vec<(Uuid, Uuid, Uuid, String)> = sqlx::query_as(
            r#"
            SELECT operasjon_id, command_id, sak_id, status::text
            FROM operasjon
            WHERE sak_id = ANY($1) OR command_id = ANY($2)
            ORDER BY operasjon_id
            FOR UPDATE NOWAIT
            "#,
        )
        .bind(&sak_uuids)
        .bind(command_ids)
        .fetch_all(&mut *tx)
        .await
        .map_err(|err| {
            if er_laas_ikke_tilgjengelig(&err) {
                FrigivelseFeil::Opptatt
            } else {
                FrigivelseFeil::Repository(
                    anyhow::Error::new(err).context("failed to lock operasjoner"),
                )
            }
        })?;

        let mut per_sak: HashMap<Uuid, Vec<Operasjonsstatus>> = HashMap::new();
        let mut operasjoner = Vec::with_capacity(rader.len());
        for (operasjon_id, command_id, sak_id, status) in rader {
            let status = les_status(&status)?;
            per_sak.entry(sak_id).or_default().push(status);
            operasjoner.push((operasjon_id, command_id, status));
        }

        let mut sperret: Vec<SkuffenSakId> = per_sak
            .iter()
            .filter(|(_, statuser)| sak_sperrer_gjenopptak(statuser))
            .map(|(sak_id, _)| SkuffenSakId::from(*sak_id))
            .collect();
        if !sperret.is_empty() {
            sperret.sort_by_key(|sak| sak.0);
            return Err(FrigivelseFeil::SakHarUavklartUtfall(sperret));
        }

        for (operasjon_id, command_id, status) in operasjoner {
            if !command_ids.contains(&command_id) {
                continue;
            }
            match vurder_operasjon(status) {
                Operasjonsgjenopptak::GjorKlar => {
                    sqlx::query(
                        r#"UPDATE operasjon
                           SET status = 'klar', neste_forsok_at = now(),
                               ferdig_at = NULL, sendt_at = NULL
                           WHERE operasjon_id = $1 AND status = 'feilet'"#,
                    )
                    .bind(operasjon_id)
                    .execute(&mut *tx)
                    .await
                    .context("failed to reopen operasjon")?;
                }
                Operasjonsgjenopptak::Fremskynd => {
                    sqlx::query(
                        r#"UPDATE operasjon SET neste_forsok_at = now()
                           WHERE operasjon_id = $1 AND status IN ('retry_venter', 'blokkert')"#,
                    )
                    .bind(operasjon_id)
                    .execute(&mut *tx)
                    .await
                    .context("failed to expedite operasjon")?;
                }
                Operasjonsgjenopptak::Urort => {}
            }
        }

        tx.commit().await.context("failed to commit retry")?;
        Ok(())
    }
}

fn les_status(status: &str) -> anyhow::Result<Operasjonsstatus> {
    Operasjonsstatus::from_code(status).context("ukjent operasjonsstatus")
}

fn er_laas_ikke_tilgjengelig(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .and_then(|db| db.code())
        .is_some_and(|code| code == LAAS_IKKE_TILGJENGELIG)
}
