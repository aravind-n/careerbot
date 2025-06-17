mod microsoft;

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    str::FromStr,
    sync::Arc,
};

use async_trait::async_trait;
use serde_json::Value;
use shared::{db::JobStore, job::Job};
use tracing::{info, warn};

use crate::collector::microsoft::MicrosoftCollector;

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
        store: Arc<dyn JobStore>,
    ) -> Result<(), Box<dyn Error>> {
        for job in jobs {
            if store.job_exists(job).await? {
                warn!(job = %job, "Skipping insert. Job exists in DB");
            }

            store.insert_job(job).await?;
            info!(job = %job, "Inserted job into db");
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
    async fn collect(&self, store: Arc<dyn JobStore>) -> Result<(), Box<dyn Error>> {
        let api_response = self.fetch_api_response().await?;
        let jobs = self.process(api_response)?;
        self.write_to_db(&jobs, store).await?;
        Ok(())
    }

    /// Returns the string identifier of a collector
    ///
    /// Used to get a name for logging purposes
    fn name(&self) -> &'static str;
}

/// Strongly typed identifier for Collectors
///
/// This enum is used to avoid using strings as keys
/// during configuration
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) enum Collector {
    /// Microsoft Careers site collector
    Microsoft,

    /// Google Careers site collector
    Google,
}

/// Type alias for factory functions that return boxed [`JobCollector`]
///
/// Allows dynamic instantiation of collectors at runtime
type CollectorFactory = Box<dyn Fn() -> Box<dyn JobCollector + Send + Sync>>;

impl Collector {
    /// Returns a list of all Collector variants
    ///
    /// Useful for test cases
    #[cfg(test)]
    fn all_variants() -> &'static [Self] {
        use Collector::*;

        // Currently only returns Microsoft since the google collector
        // hasn't been created yet
        &[Microsoft]
    }

    /// Returns a lowercase string representation of a collector
    ///
    /// Useful for logging
    pub fn as_str(&self) -> &'static str {
        match self {
            Collector::Microsoft => "microsoft",
            Collector::Google => "google",
        }
    }

    /// Parses a list of collector names to [`Collector`] variants
    ///
    /// # Arguments
    /// * `config` - A vector of collector keys as strings (Example: "microsoft", "google")
    ///
    /// # Returns
    /// A vector of [`Collector`] values
    ///
    /// # Errors
    /// Prints each failed key to stderr
    pub fn load_collector_config(config: &[String]) -> Vec<Collector> {
        config
            .iter()
            .map(|s| s.to_lowercase())
            .fold(HashSet::new(), |mut collector_set, s| {
                match s.parse::<Collector>() {
                    Ok(k) => {
                        collector_set.insert(k);
                    }
                    Err(e) => warn!(invalid_key = %s, error = %e, "Failed to parse collector key"),
                }

                collector_set
            })
            .into_iter()
            .collect()
    }

    /// Builds a map of all available Collector factory functions
    ///
    /// # Returns
    /// A map of [`Collector`] values to the corresponding factory functions
    pub fn build_factory_map() -> HashMap<Collector, CollectorFactory> {
        let mut factory_map: HashMap<Collector, CollectorFactory> = HashMap::new();

        factory_map.insert(
            Collector::Microsoft,
            Box::new(|| Box::new(MicrosoftCollector::new())),
        );

        factory_map
    }
}

impl FromStr for Collector {
    type Err = String;

    /// Parses a string into a [`Collector`] variant
    ///
    /// # Errors
    /// Returns an error string for each invalid collector key
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "microsoft" => Ok(Collector::Microsoft),
            "google" => Ok(Collector::Google),
            _ => Err(format!("Invalid Collector Name: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_as_str_microsoft() {
        assert_eq!("microsoft", Collector::Microsoft.as_str())
    }

    #[test]
    fn test_as_str_google() {
        assert_eq!("google", Collector::Google.as_str())
    }

    #[test]
    fn test_from_str_microsoft() {
        let result = Collector::from_str("microsoft");
        assert!(result.is_ok());
        assert_eq!(Collector::Microsoft, result.unwrap())
    }

    #[test]
    fn test_from_str_google() {
        let result = Collector::from_str("google");
        assert!(result.is_ok());
        assert_eq!(Collector::Google, result.unwrap());
    }

    #[test]
    fn test_from_str_invalid_value() {
        assert!(Collector::from_str("invalid").is_err())
    }

    #[test]
    fn test_load_collector_config_valid_values() {
        let config = vec![String::from("microsoft"), String::from("google")];
        let expected: HashSet<_> = vec![Collector::Microsoft, Collector::Google]
            .into_iter()
            .collect();
        let actual = Collector::load_collector_config(&config)
            .into_iter()
            .collect();

        assert_eq!(expected, actual)
    }

    #[test]
    fn test_load_collector_config_invalid_values() {
        let invalid_config = vec![String::from("invalid")];
        assert!(Collector::load_collector_config(&invalid_config).is_empty())
    }

    #[test]
    fn test_load_collector_config_mixed_values() {
        let mixed_config = vec![String::from("microsoft"), String::from("invalid")];
        let expected = vec![Collector::Microsoft];
        let actual = Collector::load_collector_config(&mixed_config);
        assert_eq!(expected, actual)
    }

    #[test]
    fn test_load_collector_config_empty() {
        assert!(Collector::load_collector_config(&[]).is_empty())
    }

    #[test]
    fn test_load_collector_config_with_duplicates() {
        let duplicate_config = vec![String::from("microsoft"), String::from("microsoft")];
        let expected = vec![Collector::Microsoft];
        assert_eq!(
            expected,
            Collector::load_collector_config(&duplicate_config)
        )
    }

    #[test]
    fn test_load_collector_config_case_insensitivity() {
        let case_insensitive_config = vec![String::from("micRosOft")];
        let expected = vec![Collector::Microsoft];
        assert_eq!(
            expected,
            Collector::load_collector_config(&case_insensitive_config)
        )
    }

    #[test]
    fn test_build_factory_map() {
        let factory_map = Collector::build_factory_map();

        assert!(
            Collector::all_variants()
                .iter()
                .all(|key| factory_map.contains_key(key))
        )
    }
}
