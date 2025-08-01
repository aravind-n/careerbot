use std::{error::Error, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use shared::{database::Database, job::Job, messaging::MessagePublisher};
use tracing::{error, info, warn};

/// Trait that defines behaviors for all job collectors
///
/// Each collector is responsible for fetching job postings
/// from a source and providing a consistent interface to
/// the ingestion engine
#[async_trait]
pub(crate) trait JobCollector: Send + Sync {
    /// Fetches an API resonse from a careers site
    ///
    /// # Returns
    /// A [`serde_json::Value`] json value
    ///
    /// # Errors
    /// Returns any errors from the http request and any
    /// errors with processing the response into a JSON
    async fn fetch_api_response(&self) -> Result<Value, Box<dyn Error>>;

    /// Processes an API response JSON into a vector of [`Job`] values
    ///
    /// # Arguments
    /// * `api_response` - A `serde-json::Value` instance
    ///
    /// # Returns
    /// A vector of [`Job`] values
    ///
    /// # Errors
    /// Any errors in building a `Job` instance
    fn process(&self, api_response: Value) -> Result<Vec<Job>, Box<dyn Error>>;

    /// Writes `Job` objects to an external database
    ///
    /// # Arguments
    /// * `jobs` - A vector of [`Job`] values
    /// * `db_pool`- An `sqlx::PgPool` Postgres pool
    ///
    /// # Errors
    /// Any errors with writing to databases
    async fn write_to_db(
        &self,
        jobs: &[Job],
        database: Arc<dyn Database>,
    ) -> Result<Vec<Job>, Box<dyn Error>> {
        let mut inserted_jobs = Vec::new();

        for job in jobs {
            match database.insert_job(job).await {
                Ok(_) => inserted_jobs.push(job.clone()),
                Err(e) => {
                    if e.to_string().contains("duplicate") {
                        warn!(job = %job, "Skipping insert. Job exists in DB");
                    } else {
                        error!(error = %e, job = %job, "Unexpected error occured while inserting job");
                    }
                }
            }

            info!(job = %job, "Inserted job into db");
        }

        Ok(inserted_jobs)
    }

    async fn publish_jobs(
        &self,
        jobs: &[Job],
        publisher: Arc<dyn MessagePublisher>,
    ) -> Result<(), Box<dyn Error>> {
        for job in jobs {
            let json = serde_json::to_value(job)?;
            publisher.publish(json).await?;
            info!(job = %job, "Published job to stream");
        }

        Ok(())
    }

    /// Executes job collection logic
    ///
    /// # Arguments
    /// * `store` - An Arc `shared::db::JobStore` object
    ///
    /// # Errors
    /// Any errors that occur in any of the called functions
    async fn collect(
        &self,
        database: Arc<dyn Database>,
        publisher: Arc<dyn MessagePublisher>,
    ) -> Result<(), Box<dyn Error>> {
        let api_response = self.fetch_api_response().await?;
        let jobs = self.process(api_response)?;
        let inserted_jobs = self.write_to_db(&jobs, database).await?;
        self.publish_jobs(&inserted_jobs, publisher).await?;

        Ok(())
    }

    /// Returns the string identifier of a collector
    ///
    /// Used to get a name for logging purposes
    fn name(&self) -> &'static str;
}
