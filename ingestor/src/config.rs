/// Configuration for the job-ingestor service
///
/// Allows the user to set the list of `Collector`
/// types to use and the delay duration before re-polling
#[derive(Debug, Clone)]
pub(crate) struct Config {
    /// A vector of enabled `Collector` names
    pub collectors: Vec<String>,

    /// The default delay between successive polling
    /// operations
    pub delay_duration: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            collectors: vec![String::from("microsoft")],
            delay_duration: 5 * 60,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn test_default_config() {
        let expected_collectors = vec![String::from("microsoft")];
        let expected_duration = 5 * 60;
        let default_config = Config::default();

        assert_eq!(expected_collectors, default_config.collectors);
        assert_eq!(expected_duration, default_config.delay_duration);
    }
}
