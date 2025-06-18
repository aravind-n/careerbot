pub mod postgres;

use std::{error::Error, sync::Arc};

use async_trait::async_trait;

use crate::job::Job;
use postgres::PostgresStore;
use tracing::error;

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

/// Initialize the database configuration
/// TODO determine which type of store to create at runtime
///
/// # Returns
/// An `Arc` `JobStore` value that handles db communications
///
/// # Errors
/// * Missing DATABASE_URL environment variable
/// * Any errors that occur when creating the db pool
pub async fn init_db(endpoint: &str) -> Result<Arc<dyn JobStore>, Box<dyn Error>> {
    let pool = PostgresStore::create_pool(endpoint)
        .await
        .inspect_err(|e| error!(error = %e, "Failed to create database pool"))?;

    let store = Arc::new(PostgresStore::new(pool));

    Ok(store)
}
