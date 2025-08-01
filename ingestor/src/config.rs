use std::{collections::HashMap, env, error::Error, sync::Arc};

use shared::{
    database::{Database, DatabaseFactory, postgres::PostgresDatabaseFactory},
    messaging::{MessagePublisher, MessagePublisherFactory, redis::RedisStreamPublisherFactory},
};
use tracing::error;

use crate::collector::{Collector, CollectorFactory};

/// Configuration for the ingestor service
///
/// Allows the user to set the list of `Collector`
/// types to use and the delay duration before re-polling
pub(crate) struct IngestorConfig {
    /// A vector of enabled `Collector` instances
    pub(crate) collectors: Vec<Collector>,

    /// The default delay between successive polling
    /// operations
    pub(crate) delay_duration: u64,

    /// A `Database` instance that is used to write jobs
    /// to a database
    pub(crate) database: Arc<dyn Database>,

    /// A `MessagePublisher` object that is used to
    /// publish jobs to a message queue
    pub(crate) message_publisher: Arc<dyn MessagePublisher>,

    /// A map of enabled `Collector` values to their associated
    /// factory functions. Used to build `JobCollector` values
    /// lazily at runtime
    pub(crate) factory_map: HashMap<Collector, CollectorFactory>,
}

impl IngestorConfig {
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
    /// - The `DATABASE_URL`, `MQUEUE_URL`, or `STREAM_KEY_JOBS` environment variables are missing or invalid.
    /// - The database connection pool cannot be created.
    /// - The Redis stream client cannot be initialized.
    pub(crate) async fn load_configuration() -> Result<Self, Box<dyn Error>> {
        let db_endpoint = env::var("DATABASE_URL")
            .inspect_err(|e| error!(error = %e, "Missing DATABASE_URL environment variable"))?;

        let mqueue_endpoint = env::var("MQUEUE_URL")
            .inspect_err(|e| error!(error = %e, "Missing MQUEUE_URL environment variable"))?;

        let stream_key = env::var("STREAM_KEY_JOBS")
            .inspect_err(|e| error!(error = %e, "Missing STREAM_KEY_JOBS environment variable"))?;

        Ok(Self {
            collectors: Self::get_enabled_collectors()?,
            delay_duration: 5 * 60,
            database: PostgresDatabaseFactory::init(&db_endpoint).await?,
            message_publisher: RedisStreamPublisherFactory::init(&mqueue_endpoint, &stream_key)
                .await?,
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
