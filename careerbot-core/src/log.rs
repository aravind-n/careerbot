use tracing_subscriber::EnvFilter;

/// Initializes the global `tracing` subscriber.
///
/// Output is JSON, level is controlled by `RUST_LOG`.
pub fn init_tracing() {
    tracing_subscriber::fmt()
        .json()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::from_default_env())
        .with_current_span(true)
        .with_span_list(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .init();
}
