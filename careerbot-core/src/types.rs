//! Shared domain types used by both the tool layer and the agent loop.
//!
//! These types live separately from `tools` and `agent` so the two modules
//! can depend on the same shape without depending on each other.

use serde::{Deserialize, Serialize};

/// A single job emitted by a per-company script on stdout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RawJob {
    pub external_id: String,
    pub title: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub posted_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A previously-seen job, returned by `CoreTools::list_known_jobs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobSummary {
    pub external_id: String,
    pub title: String,
    pub url: String,
    pub first_seen_at: String,
}

/// Hard-deny / allow filters applied with zero token cost after each
/// script run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Filters {
    #[serde(default)]
    pub title_deny: Vec<String>,
    #[serde(default)]
    pub location_allow_countries: Vec<String>,
    #[serde(default)]
    pub require_remote_or_locations: Vec<String>,
    #[serde(default)]
    pub clearance_deny: Vec<String>,
}

/// One row's worth of data for the `runs` audit table.
#[derive(Debug, Clone)]
pub struct RunResult {
    pub started_at: String,
    pub finished_at: Option<String>,
    pub exit_code: Option<i32>,
    pub new_job_count: Option<i64>,
    pub stderr_tail: Option<String>,
    pub error: Option<String>,
}

/// One row's worth of data for the `token_usage` audit table.
#[derive(Debug, Clone)]
pub struct TokenUsage {
    pub occurred_at: String,
    pub provider: String,
    pub model: Option<String>,
    pub purpose: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub company_tag: Option<String>,
}
