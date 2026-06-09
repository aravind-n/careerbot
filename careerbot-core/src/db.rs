use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{SqlitePool, migrate::Migrator};
use std::path::Path;
use std::str::FromStr;

/// Bundled migration set under `careerbot-core/migrations/`.
///
/// `sqlx::migrate!` walks the directory at compile time and embeds each
/// `.sql` file into the binary, so the daemon never needs the source
/// repo at runtime to migrate a fresh database.
pub static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

/// Open (or create) the `SQLite` database at `path` and apply pending
/// migrations.  `create_if_missing` is on so a brand-new install lands
/// directly in a usable state; foreign keys are enabled per row so the
/// notifications → jobs CASCADE actually fires.
pub async fn open(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(sqlx::Error::Io)?;
    }
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new().connect_with(opts).await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

/// In-memory database, primarily for tests. Each call returns a fresh
/// isolated pool. `SQLite`'s `sqlite::memory:` databases are private to
/// a single connection, so the pool is capped at `max_connections(1)`
/// to keep every query in the same logical database.
pub async fn open_memory() -> Result<SqlitePool, sqlx::Error> {
    let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;
    MIGRATOR.run(&pool).await?;
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migration_creates_expected_tables() {
        let pool = open_memory().await.expect("open in-memory db");
        let expected = [
            "jobs",
            "runs",
            "notifications",
            "token_usage",
            "company_state",
        ];
        for table in expected {
            let (count,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?",
            )
            .bind(table)
            .fetch_one(&pool)
            .await
            .expect("query sqlite_master");
            assert_eq!(count, 1, "expected table {table} to exist");
        }
    }

    #[tokio::test]
    async fn jobs_unique_on_company_external_id() {
        let pool = open_memory().await.expect("open in-memory db");

        sqlx::query(
            "INSERT INTO jobs (company_tag, external_id, title, url) \
             VALUES ('microsoft', 'job-1', 'SWE', 'https://x')",
        )
        .execute(&pool)
        .await
        .expect("first insert");

        let dup = sqlx::query(
            "INSERT INTO jobs (company_tag, external_id, title, url) \
             VALUES ('microsoft', 'job-1', 'SWE', 'https://x')",
        )
        .execute(&pool)
        .await;
        assert!(
            dup.is_err(),
            "duplicate (company_tag, external_id) must fail"
        );
    }

    #[tokio::test]
    async fn notifications_cascade_on_job_delete() {
        let pool = open_memory().await.expect("open in-memory db");

        sqlx::query(
            "INSERT INTO jobs (id, company_tag, external_id, title, url) \
             VALUES (1, 'microsoft', 'job-1', 'SWE', 'https://x')",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO notifications (job_id, channel, sent_at, success) \
             VALUES (1, 'os', datetime('now'), 1)",
        )
        .execute(&pool)
        .await
        .unwrap();

        sqlx::query("DELETE FROM jobs WHERE id = 1")
            .execute(&pool)
            .await
            .unwrap();

        let remaining: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM notifications")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining.0, 0, "CASCADE should remove the notification row");
    }

    #[tokio::test]
    async fn open_creates_db_file_on_disk() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("nested/db.sqlite");
        let pool = open(&path).await.expect("open on-disk db");
        assert!(path.exists());

        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM company_state")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
