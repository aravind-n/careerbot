mod job_collector;
mod microsoft;

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

use tracing::warn;

use crate::collector::microsoft::MicrosoftCollector;

pub(crate) use crate::collector::job_collector::JobCollector;

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
pub(crate) type CollectorFactory = Box<dyn Fn() -> Box<dyn JobCollector + Send + Sync>>;

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
    /// TODO move this to config.rs
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
    /// TODO investigate moving this to config.rs
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
