pub mod postgres;

use std::{error::Error, sync::Arc};

use async_trait::async_trait;

use crate::{job::Job, user::User};

/// A trait representing a storage backend.
///
/// Implementors of this trait provide methods for inserting and querying
/// records in a persistent store.
#[async_trait]
pub trait Database: Send + Sync {
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

    async fn get_interested_users_for_job(&self, job: &Job) -> Result<Vec<User>, Box<dyn Error>>;
}

#[async_trait]
pub trait DatabaseFactory: Send + Sync {
    async fn init(endpoint: &str) -> Result<Arc<dyn Database>, Box<dyn Error>>;
}
