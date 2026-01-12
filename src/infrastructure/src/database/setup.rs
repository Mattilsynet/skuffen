use lib_sql::database_config::DbPool;

pub async fn stup_database() -> Result<DbPool, sqlx::Error> {
    let pool = lib_sql::database_config::get_database_pool().await?;
    Ok(pool)
}
