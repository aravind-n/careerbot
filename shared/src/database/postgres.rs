use std::time::Duration;
use std::{error::Error, sync::Arc};

use async_trait::async_trait;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tracing::error;

use crate::{
    database::{Database, DatabaseFactory},
    job::Job,
    user::User,
};

/// A PostgreSQL-backed implementation of the `DataStore` trait.
///
/// This store provides methods to insert records and check for their
/// existence in a PostgreSQL database using SQLx.
pub struct PostgresDatabase {
    pool: PgPool,
}

pub struct PostgresDatabaseFactory;

impl PostgresDatabase {
    /// Creates a new `PostgresStore` with the given PostgreSQL connection pool.
    ///
    /// # Arguments
    ///
    /// * `pool` - A `PgPool` that will be used to execute database operations.
    ///
    /// # Returns
    ///
    /// * `PostgresStore` instance tied to the given pool.
    fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Creates a PostgreSQL connection pool using the given database URL.
    ///
    /// Configures the pool with a maximum of 10 connections and a connection
    /// acquisition timeout of 5 seconds.
    ///
    /// # Arguments
    ///
    /// * `database_url` - A string slice containing the database connection URL.
    ///
    /// # Returns
    ///
    /// * `Result<PgPool, sqlx::Error>` - A result containing the connection pool
    ///   on success, or a `sqlx::Error` if the connection fails.
    async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(5))
            .connect(database_url)
            .await?;

        Ok(pool)
    }
}

#[async_trait]
impl Database for PostgresDatabase {
    async fn insert_job(&self, job: &Job) -> Result<(), Box<dyn Error>> {
        sqlx::query!(
            r#"
            INSERT INTO jobs (
                id,
                job_portal_id,
                company_tag,
                title,
                description,
                employment_type,
                location,
                other_data,
                post_date,
                created_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            job.id,
            job.job_portal_id,
            job.company_tag,
            job.title,
            job.description,
            job.employment_type,
            job.location.as_deref(),
            job.other_data.as_deref(),
            job.post_date,
            job.created_at,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_interested_users_for_job(&self, job: &Job) -> Result<Vec<User>, Box<dyn Error>> {
        let sql = r#"
            SELECT users.* FROM users
            JOIN subscriptions on users.id = subscriptions.user_id
            WHERE subscriptions.company_tag LIKE $1
              AND (
                array_length(subscriptions.query_string, 1) IS NULL
                OR EXISTS (
                  SELECT 1
                  FROM unnest(subscriptions.query_string) AS q
                  WHERE $2 ILIKE '%' || q || '%'
                )
              )
              AND (
                array_length(subscriptions.exclude_string, 1) IS NULL
                OR NOT EXISTS (
                  SELECT 1
                  FROM unnest(subscriptions.exclude_string) AS e
                  WHERE $3 ILIKE '%' || e || '%'
                )
              )
        "#;

        let result: Vec<User> = sqlx::query_as(sql)
            .bind(format!("%{}%", job.company_tag))
            .bind(job.title.to_lowercase())
            .bind(job.title.to_lowercase())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed to retrieve subscriptions");
                e
            })?;

        Ok(result)
    }
}

#[async_trait]
impl DatabaseFactory for PostgresDatabaseFactory {
    async fn init(endpoint: &str) -> Result<Arc<dyn Database>, Box<dyn Error>> {
        let pool = PostgresDatabase::create_pool(endpoint)
            .await
            .inspect_err(|e| error!(error = %e, "Failed to create database pool"))?;

        let store = Arc::new(PostgresDatabase::new(pool));

        Ok(store)
    }
}
