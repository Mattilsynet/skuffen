use anyhow::Result;
use sqlx::PgPool;
use tokio::time::{sleep, Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbSakState {
    pub status: String,
    pub opprettet: bool,
    pub saksnummer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbJournalpostState {
    pub journalfoert: bool,
    pub avskrevet: bool,
    pub ekspedert: bool,
    pub har_feilede_dokumenter: bool,
    pub med_utsending: bool,
    pub journalposttype: char,
    pub journalpostnummer: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbDokumentState {
    pub lagt_til: bool,
    pub irrecoverable_feil: bool,
}

pub async fn fetch_sak_state(pool: &PgPool, sak_id: Uuid) -> Result<Option<DbSakState>> {
    let row: Option<(String, bool, Option<String>)> = sqlx::query_as(
        r#"
        SELECT status, opprettet, saksnummer
        FROM sak_state
        WHERE sak_id = $1
        "#,
    )
    .bind(sak_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(status, opprettet, saksnummer)| DbSakState {
        status,
        opprettet,
        saksnummer,
    }))
}

pub async fn fetch_sak_state_for_client_reference(
    pool: &PgPool,
    client_reference: Uuid,
) -> Result<Option<DbSakState>> {
    let skuffen_id: Option<Uuid> = sqlx::query_scalar(
        r#"
        SELECT skuffen_id
        FROM id_mapping
        WHERE client_reference = $1
        "#,
    )
    .bind(client_reference)
    .fetch_optional(pool)
    .await?;

    match skuffen_id {
        Some(skuffen_id) => fetch_sak_state(pool, skuffen_id).await,
        None => Ok(None),
    }
}

pub async fn fetch_journalpost_state(
    pool: &PgPool,
    journalpost_id: Uuid,
) -> Result<Option<DbJournalpostState>> {
    let row: Option<(bool, bool, bool, bool, bool, String, Option<i32>)> = sqlx::query_as(
        r#"
        SELECT journalfoert, avskrevet, ekspedert, har_feilede_dokumenter, med_utsending, journalposttype, journalpostnummer
        FROM journalpost_state
        WHERE journalpost_id = $1
        "#,
    )
    .bind(journalpost_id)
    .fetch_optional(pool)
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
        )| DbJournalpostState {
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

pub async fn fetch_journalpost_state_for_client_reference(
    pool: &PgPool,
    client_reference: Uuid,
) -> Result<Option<DbJournalpostState>> {
    let skuffen_id: Option<Uuid> = sqlx::query_scalar(
        r#"
        SELECT skuffen_id
        FROM id_mapping
        WHERE client_reference = $1
        "#,
    )
    .bind(client_reference)
    .fetch_optional(pool)
    .await?;

    match skuffen_id {
        Some(skuffen_id) => fetch_journalpost_state(pool, skuffen_id).await,
        None => Ok(None),
    }
}

pub async fn fetch_dokument_state(
    pool: &PgPool,
    dokument_id: Uuid,
) -> Result<Option<DbDokumentState>> {
    let row: Option<(bool, bool)> = sqlx::query_as(
        r#"
        SELECT lagt_til, irrecoverable_feil
        FROM dokument_state
        WHERE dokument_id = $1
        "#,
    )
    .bind(dokument_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(lagt_til, irrecoverable_feil)| DbDokumentState {
        lagt_til,
        irrecoverable_feil,
    }))
}

pub async fn fetch_dokument_state_for_client_reference(
    pool: &PgPool,
    client_reference: Uuid,
) -> Result<Option<DbDokumentState>> {
    let skuffen_id: Option<Uuid> = sqlx::query_scalar(
        r#"
        SELECT skuffen_id
        FROM id_mapping
        WHERE client_reference = $1
        "#,
    )
    .bind(client_reference)
    .fetch_optional(pool)
    .await?;

    match skuffen_id {
        Some(skuffen_id) => fetch_dokument_state(pool, skuffen_id).await,
        None => Ok(None),
    }
}

pub async fn fetch_command_execution_status(
    pool: &PgPool,
    command_id: Uuid,
) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT status
        FROM command_execution
        WHERE command_id = $1
        "#,
    )
    .bind(command_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(status,)| status))
}

pub async fn wait_for_command_execution(
    pool: &PgPool,
    command_id: Uuid,
    timeout: Duration,
) -> Result<String> {
    let deadline = Instant::now() + timeout;
    let mut last_seen_status: Option<String> = None;
    loop {
        if let Some(status) = fetch_command_execution_status(pool, command_id).await? {
            last_seen_status = Some(status.clone());
            if matches!(status.as_str(), "ok" | "blocked" | "error") {
                return Ok(status);
            }
        }
        if Instant::now() >= deadline {
            let row: Option<(String, i32, Option<String>)> = sqlx::query_as(
                r#"
                SELECT status, attempts, last_error
                FROM command_execution
                WHERE command_id = $1
                "#,
            )
            .bind(command_id)
            .fetch_optional(pool)
            .await?;

            anyhow::bail!(
                "Timed out waiting for terminal command execution status. last_seen_status={:?}, row={:?}",
                last_seen_status,
                row
            );
        }
        sleep(Duration::from_millis(200)).await;
    }
}

pub async fn wait_for_command_execution_all(
    pool: &PgPool,
    command_ids: impl IntoIterator<Item = Uuid>,
    timeout: Duration,
) -> Result<()> {
    for command_id in command_ids {
        let _ = wait_for_command_execution(pool, command_id, timeout).await?;
    }
    Ok(())
}

pub async fn insert_id_mapping(
    pool: &PgPool,
    skuffen_id: Uuid,
    entity_type: &str,
    client_reference: Uuid,
    arkiv_id: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO id_mapping (skuffen_id, entity_type, client_reference, arkiv_id)
        VALUES ($1, $2::entity_type, $3, $4)
        ON CONFLICT (client_reference) WHERE client_reference IS NOT NULL DO NOTHING
        "#,
    )
    .bind(skuffen_id)
    .bind(entity_type)
    .bind(client_reference)
    .bind(arkiv_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn insert_arkiv_id_mapping(
    pool: &PgPool,
    skuffen_id: Uuid,
    entity_type: &str,
    arkiv_id: &str,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO id_mapping (skuffen_id, entity_type, client_reference, arkiv_id)
        VALUES ($1, $2::entity_type, NULL, $3)
        ON CONFLICT (entity_type, arkiv_id) DO NOTHING
        "#,
    )
    .bind(skuffen_id)
    .bind(entity_type)
    .bind(arkiv_id)
    .execute(pool)
    .await?;

    Ok(())
}
