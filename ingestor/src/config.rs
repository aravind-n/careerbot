use std::{collections::HashMap, env, error::Error, sync::Arc};

use shared::{
    database::{JobStore, postgres::PostgresStore},
    stream::{StreamPublisher, redis::RedisStreamPublisher},
};
use tracing::error;
use tracing_subscriber::EnvFilter;

use crate::collector::{Collector, CollectorFactory};

/// Configuration for the job-ingestor service
///
/// Allows the user to set the list of `Collector`
/// types to use and the delay duration before re-polling
pub(crate) struct Config {
    /// A vector of enabled `Collector` instances
    pub(crate) collectors: Vec<Collector>,

    /// The default delay between successive polling
    /// operations
    pub(crate) delay_duration: u64,

    /// A `JobStore` instance that is used to write jobs
    /// to a database
    pub(crate) store: Arc<dyn JobStore>,

    /// A `StreamPublisher` object that is used to
    /// publish jobs to a data stream
    pub(crate) publisher: Arc<dyn StreamPublisher>,

    /// A map of enabled `Collector` values to their associated
    /// factory functions. Used to build `JobCollector` values
    /// lazily at runtime
    pub(crate) factory_map: HashMap<Collector, CollectorFactory>,
}

impl Config {
    /// Initializes `tracing_subscriber` configuration
    ///
    /// This allows the package to output logs using
    /// the `tracing` crate
    pub(crate) fn init_tracing() {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(EnvFilter::from_default_env())
            .with_current_span(true)
            .with_span_list(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .init();
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
    async fn init_db() -> Result<Arc<dyn JobStore>, Box<dyn Error>> {
        let db_url = env::var("DATABASE_URL")
            .inspect_err(|e| error!(error = %e, "Missing DATABASE_URL environment variable"))?;

        let pool = PostgresStore::create_pool(&db_url)
            .await
            .inspect_err(|e| error!(error = %e, "Failed to create database pool"))?;

        let store = Arc::new(PostgresStore::new(pool));

        Ok(store)
    }

    /// Initialize the stream publisher configuration
    /// TODO determine which type of stream to create at runtime
    ///
    /// # Returns
    /// An `Arc` `StreamPublisher` value that handles data stream publishing
    ///
    /// # Errors
    /// * Missing STREAM_URL environment variable
    /// * Missing JOB_STREAM_KEY environment variable
    /// * Any errors that occur while opening a connection to the stream
    fn init_stream_publisher() -> Result<Arc<dyn StreamPublisher>, Box<dyn Error>> {
        let stream_url = env::var("STREAM_URL")
            .inspect_err(|e| error!(error = %e, "Missing STREAM_URL environment variable"))?;

        let stream_key = env::var("STREAM_KEY_JOBS")
            .inspect_err(|e| error!(error = %e, "Missing STREAM_KEY_JOBS environment variable"))?;

        let stream_client = RedisStreamPublisher::get_client(&stream_url)
            .inspect_err(|e| error!(error = %e, "Failed to open stream connection"))?;

        Ok(Arc::new(RedisStreamPublisher::new(
            stream_client,
            &stream_key,
        )))
    }

    /// Initializes the list of enabled collectors
    ///
    /// # Returns
    /// A vector of `Collector` objects
    ///
    /// # Errors
    /// * Currently none. In the future, will return any errors with runtime configuration
    fn get_enabled_collectors() -> Result<Vec<Collector>, Box<dyn Error>> {
        // TODO wire this to be configurable at run time
        let enabled_collectors = vec![String::from("microsoft")];

        Ok(Collector::load_collector_config(&enabled_collectors))
    }

    /// Asynchronously initializes a fully configured `Config` instance for the job-ingestor service.
    ///
    /// This function sets up all necessary service components, including:
    /// - The database store (`JobStore`) used to persist job data.
    /// - The stream publisher (`StreamPublisher`) used to emit job events to a Redis stream.
    /// - A map of available job collectors used to fetch job listings from different sources.
    ///
    /// # Returns
    ///
    /// A `Result` containing a fully constructed `Config` instance if all components are
    /// successfully initialized.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - The `DATABASE_URL`, `STREAM_URL`, or `JOB_STREAM_KEY` environment variables are missing or invalid.
    /// - The database connection pool cannot be created.
    /// - The Redis stream client cannot be initialized.
    pub(crate) async fn initialize() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            collectors: Self::get_enabled_collectors()?,
            delay_duration: 5 * 60,
            store: Self::init_db().await?,
            publisher: Self::init_stream_publisher()?,
            factory_map: Collector::build_factory_map(),
        })
    }
}

// TODO implement unit tests for the configuration
// #[cfg(test)]
// mod tests {
//     use super::Config;

//     #[test]
//     fn test_default_config() {
//         let expected_collectors = vec![String::from("microsoft")];
//         let expected_duration = 5 * 60;
//         let default_config = Config::initialize().await;

//         assert_eq!(expected_collectors, default_config.collectors);
//         assert_eq!(expected_duration, default_config.delay_duration);
//     }
// }
