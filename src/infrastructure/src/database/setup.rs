use lib_sql::database_config::DbPool;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

pub async fn stup_database() -> Result<DbPool, sqlx::Error> {
    let pool = lib_sql::database_config::get_database_pool().await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &DbPool) -> Result<(), sqlx::Error> {
    MIGRATOR.run(pool).await?;
    Ok(())
}
