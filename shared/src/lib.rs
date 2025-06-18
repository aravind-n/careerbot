use tokio::signal::unix::{SignalKind, signal};
use tracing::info;

pub mod database;
pub mod job;
pub mod log;
pub mod messaging;
pub mod user;

#[cfg(test)]
pub mod mock;

/// Interrupt handler for shutdown events
///
/// Currently handles SIGTERM and SIGINT events
pub async fn shutdown_signal() {
    let mut sigterm = signal(SignalKind::terminate()).unwrap();

    tokio::select! {
        _ = sigterm.recv() => {
            info!("INFO: Received SIGTERM");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("INFO: Received SIGINT");
        }
    }
}
