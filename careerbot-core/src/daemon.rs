//! Background process: HTTP IPC over a Unix domain socket, plus
//! (in later phases) the per-company scheduler and notification
//! dispatch. Follows the "new-style daemon" pattern — foreground
//! process, no fork, no PID file, supervisor-managed lifecycle.

pub mod ipc_client;
pub mod scheduler;

use crate::runtime::Runtime;
use crate::shutdown_signal;
use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Router, serve};
use scheduler::Scheduler;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;
use tracing::{info, warn};

#[derive(Debug)]
pub enum DaemonError {
    Io(std::io::Error),
    AlreadyRunning { socket: PathBuf },
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{}", e),
            Self::AlreadyRunning { socket } => write!(
                f,
                "another careerbot daemon is already listening on {}",
                socket.display()
            ),
        }
    }
}

impl std::error::Error for DaemonError {}

impl From<std::io::Error> for DaemonError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// `GET /status` payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub version: String,
    pub started_at: String,
    pub socket_path: String,
}

#[derive(Clone)]
struct DaemonState {
    #[allow(dead_code)]
    runtime: Arc<Runtime>,
    shutdown: Arc<Notify>,
    started_at: chrono::DateTime<chrono::Utc>,
    socket_path: PathBuf,
    scheduler: Arc<Scheduler>,
}

#[derive(Debug, Deserialize)]
struct RunNowParams {
    company: Option<String>,
}

/// Bind the IPC socket and serve until SIGINT/SIGTERM (or `POST
/// /shutdown`). Cleans up the socket file on the way out.
pub async fn run(runtime: Arc<Runtime>) -> Result<(), DaemonError> {
    let socket_path = runtime.paths.socket_path();

    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    ensure_socket_unoccupied(&socket_path).await?;

    let listener = UnixListener::bind(&socket_path)?;
    info!(socket = %socket_path.display(), "daemon listening");

    let shutdown = Arc::new(Notify::new());
    let scheduler = Scheduler::start(runtime.clone(), shutdown.clone()).await?;
    let state = DaemonState {
        runtime,
        shutdown,
        started_at: chrono::Utc::now(),
        socket_path: socket_path.clone(),
        scheduler,
    };

    let router = Router::new()
        .route("/status", get(handle_status))
        .route("/shutdown", post(handle_shutdown))
        .route("/run-now", post(handle_run_now))
        .with_state(state.clone());

    // Translate either SIGINT/SIGTERM or `POST /shutdown` into the
    // single graceful-shutdown future the server consumes.
    let shutdown = state.shutdown.clone();
    let shutdown_for_signal = state.shutdown.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        shutdown_for_signal.notify_waiters();
    });

    let serve_result = serve(listener, router)
        .with_graceful_shutdown(async move {
            shutdown.notified().await;
        })
        .await;

    // Always try to clean up the socket file, even if serve errored.
    let _ = tokio::fs::remove_file(&socket_path).await;

    info!("daemon stopped");
    Ok(serve_result?)
}

async fn ensure_socket_unoccupied(socket_path: &std::path::Path) -> Result<(), DaemonError> {
    if !socket_path.exists() {
        return Ok(());
    }
    match UnixStream::connect(socket_path).await {
        Ok(_) => Err(DaemonError::AlreadyRunning {
            socket: socket_path.to_path_buf(),
        }),
        Err(e) => {
            warn!(
                socket = %socket_path.display(),
                error = %e,
                "removing stale socket from prior unclean shutdown",
            );
            tokio::fs::remove_file(socket_path).await?;
            Ok(())
        }
    }
}

async fn handle_status(State(state): State<DaemonState>) -> Json<StatusResponse> {
    Json(StatusResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
        started_at: state.started_at.to_rfc3339(),
        socket_path: state.socket_path.display().to_string(),
    })
}

async fn handle_shutdown(State(state): State<DaemonState>) -> StatusCode {
    state.shutdown.notify_waiters();
    StatusCode::ACCEPTED
}

async fn handle_run_now(
    State(state): State<DaemonState>,
    Query(params): Query<RunNowParams>,
) -> StatusCode {
    let ok = state.scheduler.poke(params.company.as_deref()).await;
    if ok {
        StatusCode::ACCEPTED
    } else {
        StatusCode::NOT_FOUND
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use std::time::Duration;
    use tempfile::TempDir;

    async fn spawn_daemon(dir: &TempDir) -> (PathBuf, tokio::task::JoinHandle<()>) {
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        // Override socket path explicitly so we don't accidentally hit
        // $XDG_RUNTIME_DIR on Linux test runners.
        let socket = dir.path().join("careerbot.sock");
        let runtime = Arc::new(Runtime::open_at(paths.clone()).await.unwrap());
        let socket_for_handle = socket.clone();
        let handle = tokio::spawn(async move {
            run_with_socket(runtime, socket_for_handle).await.unwrap();
        });
        // Wait for the socket to appear; the daemon may not have bound yet.
        for _ in 0..50 {
            if socket.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        (socket, handle)
    }

    /// Same as `run` but with an explicit socket path so tests don't
    /// touch `$XDG_RUNTIME_DIR`.
    async fn run_with_socket(
        runtime: Arc<Runtime>,
        socket_path: PathBuf,
    ) -> Result<(), DaemonError> {
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        ensure_socket_unoccupied(&socket_path).await?;

        let listener = UnixListener::bind(&socket_path)?;
        let shutdown = Arc::new(Notify::new());
        let scheduler = Scheduler::start(runtime.clone(), shutdown.clone()).await?;
        let state = DaemonState {
            runtime,
            shutdown,
            started_at: chrono::Utc::now(),
            socket_path: socket_path.clone(),
            scheduler,
        };
        let router = Router::new()
            .route("/status", get(handle_status))
            .route("/shutdown", post(handle_shutdown))
            .route("/run-now", post(handle_run_now))
            .with_state(state.clone());

        let shutdown = state.shutdown.clone();
        let serve_result = serve(listener, router)
            .with_graceful_shutdown(async move {
                shutdown.notified().await;
            })
            .await;
        let _ = tokio::fs::remove_file(&socket_path).await;
        Ok(serve_result?)
    }

    #[tokio::test]
    async fn status_endpoint_returns_metadata() {
        let dir = TempDir::new().unwrap();
        let (socket, handle) = spawn_daemon(&dir).await;

        let (status, body) = ipc_client::request(&socket, "GET", "/status", &[])
            .await
            .unwrap();
        assert_eq!(status, 200);
        let parsed: StatusResponse = serde_json::from_str(&body).unwrap();
        assert!(!parsed.version.is_empty());
        assert!(parsed.started_at.contains('T'));
        assert_eq!(parsed.socket_path, socket.display().to_string());

        // Stop the daemon cleanly.
        let _ = ipc_client::request(&socket, "POST", "/shutdown", &[]).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }

    #[tokio::test]
    async fn shutdown_endpoint_triggers_clean_exit() {
        let dir = TempDir::new().unwrap();
        let (socket, handle) = spawn_daemon(&dir).await;

        let (status, _) = ipc_client::request(&socket, "POST", "/shutdown", &[])
            .await
            .unwrap();
        assert_eq!(status, 202);

        let finished = tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("daemon did not exit after /shutdown");
        finished.unwrap();
        // Socket file was cleaned up on exit.
        assert!(!socket.exists(), "socket file should be removed on exit");
    }

    #[tokio::test]
    async fn stale_socket_is_replaced() {
        let dir = TempDir::new().unwrap();
        let socket = dir.path().join("careerbot.sock");
        // Pre-create a stale socket file that no one is listening on.
        std::fs::write(&socket, b"stale").unwrap();
        assert!(socket.exists());

        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        let runtime = Arc::new(Runtime::open_at(paths).await.unwrap());
        let socket_for_run = socket.clone();
        let handle = tokio::spawn(async move {
            run_with_socket(runtime, socket_for_run).await.unwrap();
        });
        for _ in 0..50 {
            if let Ok(stream) = UnixStream::connect(&socket).await {
                drop(stream);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let (status, _) = ipc_client::request(&socket, "POST", "/shutdown", &[])
            .await
            .unwrap();
        assert_eq!(status, 202);
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }

    #[tokio::test]
    async fn second_daemon_refuses_to_start() {
        let dir = TempDir::new().unwrap();
        let (socket, handle) = spawn_daemon(&dir).await;

        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        let runtime = Arc::new(Runtime::open_at(paths).await.unwrap());
        let err = run_with_socket(runtime, socket.clone()).await.unwrap_err();
        assert!(matches!(err, DaemonError::AlreadyRunning { .. }));

        let _ = ipc_client::request(&socket, "POST", "/shutdown", &[]).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
    }
}
