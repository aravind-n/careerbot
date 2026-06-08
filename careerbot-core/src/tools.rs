//! In-process tool layer. The agent calls into a `CoreTools` instance
//! (either directly via [`AnthropicApiDriver`](crate::agent::anthropic_api)
//! or over the MCP transport for subprocess drivers) to read/write the
//! profile, run per-company scripts, and persist audit rows.
//!
//! The struct is intentionally just a bag of dependencies plus methods —
//! it has no inner state of its own beyond the pool, paths, and HTTP
//! client. Method implementations land in the next commit.

use crate::paths::Paths;
use sqlx::SqlitePool;
use std::sync::Arc;

#[derive(Debug)]
pub enum ToolError {
    Io(std::io::Error),
    Db(sqlx::Error),
    Http(reqwest::Error),
    Json(serde_json::Error),
    Script { stderr: String, exit_code: i32 },
    InvalidNdjson(String),
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{}", e),
            Self::Db(e) => write!(f, "{}", e),
            Self::Http(e) => write!(f, "{}", e),
            Self::Json(e) => write!(f, "{}", e),
            Self::Script { stderr, exit_code } => {
                write!(f, "script exited {}: {}", exit_code, stderr)
            }
            Self::InvalidNdjson(s) => write!(f, "invalid NDJSON: {}", s),
        }
    }
}

impl std::error::Error for ToolError {}

impl From<std::io::Error> for ToolError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<sqlx::Error> for ToolError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

impl From<reqwest::Error> for ToolError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

impl From<serde_json::Error> for ToolError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

pub struct CoreTools {
    db: Arc<SqlitePool>,
    paths: Paths,
    http: reqwest::Client,
}

impl CoreTools {
    pub fn new(db: Arc<SqlitePool>, paths: Paths) -> Self {
        Self {
            db,
            paths,
            http: reqwest::Client::new(),
        }
    }

    pub fn db(&self) -> &SqlitePool {
        &self.db
    }

    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }
}
