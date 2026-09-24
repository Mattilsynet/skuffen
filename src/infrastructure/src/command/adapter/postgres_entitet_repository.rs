use anyhow::{Context, Result};
use application::command::ports::entitet_port::{Entitet, EntitetRepository, NyEntitet};
use async_trait::async_trait;
use domain::eksekvering::operasjon::EntitetType;
use lib_sql::database_config::DbPool;
use uuid::Uuid;

/// Identitetstabellen (SKU-0016 R11). Master for `skuffen_id`.
pub struct PostgresEntitetRepository {
    pool: DbPool,
}

impl PostgresEntitetRepository {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

pub(crate) fn entitet_type_kode(entitet_type: EntitetType) -> &'static str {
    entitet_type.as_code()
}

pub(crate) fn entitet_type_fra_kode(kode: &str) -> Result<EntitetType> {
    match kode {
        "sak" => Ok(EntitetType::Sak),
        "journalpost" => Ok(EntitetType::Journalpost),
        "dokument" => Ok(EntitetType::Dokument),
        other => Err(anyhow::anyhow!("ukjent entitet_type: {other}")),
    }
}

#[async_trait]
impl EntitetRepository for PostgresEntitetRepository {
    async fn registrer(&self, entitet: NyEntitet) -> Result<Uuid> {
        let Some(client_reference) = entitet.client_reference else {
            // Uten client_reference finnes ingen naturlig nøkkel å være
            // idempotent på; da må arkiv_id bære identiteten.
            let arkiv_id = entitet
                .arkiv_id
                .as_deref()
                .context("entitet krever client_reference eller arkiv_id")?;
            return self
                .hent_eller_opprett_for_arkiv_id(entitet.entitet_type, arkiv_id)
                .await;
        };

        // Eksisterende rad vinner. Det gjør at en replay etter dispatch-feil
        // gjenbruker id-ene fra første forsøk i stedet for å minte nye.
        let skuffen_id: Uuid = sqlx::query_scalar(
            r#"
            WITH ny AS (
                INSERT INTO entitet (skuffen_id, entitet_type, client_reference, arkiv_id)
                VALUES ($1, $2::entitet_type, $3, $4)
                ON CONFLICT (client_reference) DO NOTHING
                RETURNING skuffen_id
            )
            SELECT skuffen_id FROM ny
            UNION ALL
            SELECT skuffen_id FROM entitet WHERE client_reference = $3
            LIMIT 1
            "#,
        )
        .bind(entitet.skuffen_id)
        .bind(entitet_type_kode(entitet.entitet_type))
        .bind(client_reference)
        .bind(entitet.arkiv_id.as_deref())
        .fetch_one(&self.pool)
        .await
        .context("failed to register entitet")?;

        Ok(skuffen_id)
    }

    async fn hent_for_client_reference(&self, client_reference: Uuid) -> Result<Option<Entitet>> {
        let rad: Option<(Uuid, String, Option<Uuid>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT skuffen_id, entitet_type::text, client_reference, arkiv_id
            FROM entitet
            WHERE client_reference = $1
            "#,
        )
        .bind(client_reference)
        .fetch_optional(&self.pool)
        .await
        .context("failed to load entitet by client reference")?;

        rad.map(|(skuffen_id, entitet_type, client_reference, arkiv_id)| {
            Ok(Entitet {
                skuffen_id,
                entitet_type: entitet_type_fra_kode(&entitet_type)?,
                client_reference,
                arkiv_id,
            })
        })
        .transpose()
    }

    async fn hent_for_arkiv_id(
        &self,
        entitet_type: EntitetType,
        arkiv_id: &str,
    ) -> Result<Option<Entitet>> {
        let rad: Option<(Uuid, String, Option<Uuid>, Option<String>)> = sqlx::query_as(
            r#"
            SELECT skuffen_id, entitet_type::text, client_reference, arkiv_id
            FROM entitet
            WHERE entitet_type = $1::entitet_type AND arkiv_id = $2
            "#,
        )
        .bind(entitet_type_kode(entitet_type))
        .bind(arkiv_id)
        .fetch_optional(&self.pool)
        .await
        .context("failed to look up entitet by arkiv id")?;

        rad.map(|(skuffen_id, entitet_type, client_reference, arkiv_id)| {
            Ok(Entitet {
                skuffen_id,
                entitet_type: entitet_type_fra_kode(&entitet_type)?,
                client_reference,
                arkiv_id,
            })
        })
        .transpose()
    }

    async fn hent_eller_opprett_for_arkiv_id(
        &self,
        entitet_type: EntitetType,
        arkiv_id: &str,
    ) -> Result<Uuid> {
        let skuffen_id: Uuid = sqlx::query_scalar(
            r#"
            WITH ny AS (
                INSERT INTO entitet (skuffen_id, entitet_type, arkiv_id)
                VALUES ($1, $2::entitet_type, $3)
                ON CONFLICT (entitet_type, arkiv_id) DO NOTHING
                RETURNING skuffen_id
            )
            SELECT skuffen_id FROM ny
            UNION ALL
            SELECT skuffen_id FROM entitet
            WHERE entitet_type = $2::entitet_type AND arkiv_id = $3
            LIMIT 1
            "#,
        )
        .bind(Uuid::now_v7())
        .bind(entitet_type_kode(entitet_type))
        .bind(arkiv_id)
        .fetch_one(&self.pool)
        .await
        .context("failed to resolve entitet by arkiv id")?;

        Ok(skuffen_id)
    }

    async fn hent_arkiv_id(&self, skuffen_id: Uuid) -> Result<Option<String>> {
        sqlx::query_scalar("SELECT arkiv_id FROM entitet WHERE skuffen_id = $1")
            .bind(skuffen_id)
            .fetch_optional(&self.pool)
            .await
            .context("failed to load arkiv id")
            .map(Option::flatten)
    }
}
