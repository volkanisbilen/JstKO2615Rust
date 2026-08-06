//! # ko-db
//!
//! Database access layer for the Knight Online Rust server.
//!
//! Uses PostgreSQL via `sqlx` with compile-time checked queries.
//! All database access follows the repository pattern — handlers
//! must never contain raw SQL.

pub mod models;
pub mod repositories;

/// Database connection pool type alias.
pub type DbPool = sqlx::PgPool;

/// Establish a connection pool to the PostgreSQL database.
///
/// Pool is configured for game server workload:
/// - 50 max connections (handles concurrent handler DB access + background tasks)
/// - 10s acquire timeout (prevents indefinite stalls under pool exhaustion)
/// - 30min idle timeout (returns unused connections to free DB resources)
///
/// # Errors
///
/// Returns an error if the connection string is invalid or the
/// database is unreachable.
pub async fn create_pool(database_url: &str) -> Result<DbPool, sqlx::Error> {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(50)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .idle_timeout(std::time::Duration::from_secs(1800))
        .connect(database_url)
        .await
}

/// Run pending database migrations from the `migrations/` directory.
///
/// Uses runtime file-based migration instead of compile-time embedding
/// to avoid inflating the binary with 300+ MB of SQL seed data.
/// Defaults to the repository root `migrations/` directory; `MIGRATIONS_DIR`
/// can override it for packaged deployments.
pub async fn run_migrations(pool: &DbPool) -> Result<(), sqlx::migrate::MigrateError> {
    let migration_dir = std::env::var_os("MIGRATIONS_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../migrations")
        });

    sqlx::migrate::Migrator::new(migration_dir)
        .await?
        .run(pool)
        .await
}
