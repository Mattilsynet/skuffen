use lib_sql::database_config::DbPool;
use sqlx::Executor;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub async fn setup_database() -> Result<DbPool, sqlx::Error> {
    let pool = lib_sql::database_config::get_database_pool().await?;
    Ok(pool)
}

/// Migrasjonene serialiseres av sqlx' advisory lock (SKU-0022 R4). Uten
/// `lock_timeout` venter en instans i det stille bak en fastlåst lås, og
/// oppstart henger i stedet for å feile.
pub async fn run_migrations(pool: &DbPool) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    connection.execute("SET lock_timeout = '60s'").await?;
    // `run` er generisk over `Acquire<'a>`, og den bindingen blir ikke
    // `Send`-general nok gjennom en async fn. `run_direct` tar connectionen
    // direkte og gjør det samme.
    MIGRATOR.run_direct(None, &mut *connection, false).await?;
    Ok(())
}
