use tracing_subscriber::EnvFilter;

/// Initializes `tracing_subscriber` configuration
///
/// This allows the package to output logs using
/// the `tracing` crate
pub fn init_tracing() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .with_current_span(true)
        .with_span_list(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .init();
}
