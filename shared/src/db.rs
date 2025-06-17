use std::error::Error;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::job::Job;

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
pub async fn create_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await?;

    Ok(pool)
}

/// A trait representing a storage backend for job entries.
///
/// Implementors of this trait provide methods for inserting and querying
/// job records in a persistent store.
#[async_trait]
pub trait JobStore: Send + Sync {
    /// Inserts a job into the store.
    ///
    /// If the job already exists, implementations may choose to ignore it or
    /// update the existing record, depending on design.
    ///
    /// # Arguments
    ///
    /// * `job` - A reference to the `Job` to be stored.
    ///
    /// # Returns
    ///
    /// * `Result<(), Box<dyn Error>>` - A result indicating success or failure.
    async fn insert_job(&self, job: &Job) -> Result<(), Box<dyn Error>>;

    /// Checks whether a given job already exists in the store.
    ///
    /// This is typically used to avoid inserting duplicates.
    ///
    /// # Arguments
    ///
    /// * `job` - A reference to the `Job` to check for existence.
    ///
    /// # Returns
    ///
    /// * `Result<bool, Box<dyn Error>>` - A result containing `true` if the job exists,
    ///   `false` otherwise, or an error if the check fails.
    async fn job_exists(&self, job: &Job) -> Result<bool, Box<dyn Error>>;
}
