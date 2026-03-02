use lib_sql::database_config::DbPool;

pub async fn stup_database() -> Result<DbPool, sqlx::Error> {
    let pool = lib_sql::database_config::get_database_pool().await?;
    Ok(pool)
}

pub async fn run_migrations(pool: &DbPool) -> Result<(), sqlx::Error> {
    let root = std::env::current_dir()?;
    let migrations_path = root.join("src").join("infrastructure").join("migrations");
    let migrator = sqlx::migrate::Migrator::new(migrations_path).await?;
    migrator.run(pool).await?;
    Ok(())
}
