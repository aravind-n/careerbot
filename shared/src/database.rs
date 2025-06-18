pub mod postgres;

use std::error::Error;

use async_trait::async_trait;

use crate::job::Job;

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
