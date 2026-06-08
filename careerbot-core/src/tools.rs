//! In-process tool layer. The agent calls into a `CoreTools` instance
//! (either directly via [`AnthropicApiDriver`](crate::agent::anthropic_api)
//! or over the MCP transport for subprocess drivers) to read/write the
//! profile, run per-company scripts, and persist audit rows.
//!
//! Method bodies are deliberately thin and side-effect-only: the daemon
//! and CLI compose them; this module makes no policy decisions.

use crate::paths::Paths;
use crate::types::{Filters, JobSummary, RawJob, RunResult, TokenUsage};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;

/// Default HTTP timeout used by [`CoreTools::fetch_url`] and shared by
/// other in-process drivers that build a [`default_http_client`].
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Default wall-time cap on a single per-company script run.  Matches the
/// save-time verification limit in PLAN.md §7.
pub const DEFAULT_SCRIPT_TIMEOUT: Duration = Duration::from_secs(60);

/// Build the canonical reqwest client with the default timeout applied.
/// Falls back to `reqwest::Client::new()` if the builder fails (e.g.
/// missing platform certs); the daemon will surface the per-call error
/// from there.
pub fn default_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(DEFAULT_HTTP_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

#[derive(Debug)]
pub enum ToolError {
    Io(std::io::Error),
    Db(sqlx::Error),
    Http(reqwest::Error),
    Json(serde_json::Error),
    Script { stderr: String, exit_code: i32 },
    InvalidNdjson(String),
    InvalidCompany(String),
    ScriptTimeout(Duration),
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
            Self::InvalidCompany(s) => write!(f, "invalid company name {:?}", s),
            Self::ScriptTimeout(d) => write!(f, "script timed out after {:?}", d),
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
    /// Argv prefix used by `run_script` — typically `["uv", "run"]`.
    /// Tests override to `["python3"]` so they don't depend on `uv`.
    script_runner: Vec<String>,
    script_timeout: Duration,
}

impl CoreTools {
    pub fn new(db: Arc<SqlitePool>, paths: Paths) -> Self {
        Self::with_script_runner(db, paths, vec!["uv".into(), "run".into()])
    }

    pub fn with_script_runner(
        db: Arc<SqlitePool>,
        paths: Paths,
        script_runner: Vec<String>,
    ) -> Self {
        assert!(!script_runner.is_empty(), "script_runner must be non-empty");
        Self {
            db,
            paths,
            http: default_http_client(),
            script_runner,
            script_timeout: DEFAULT_SCRIPT_TIMEOUT,
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

    /// HTTP GET. Optional headers are merged onto the request.
    pub async fn fetch_url(
        &self,
        url: &str,
        headers: Option<HashMap<String, String>>,
    ) -> Result<String, ToolError> {
        let mut req = self.http.get(url);
        if let Some(headers) = headers {
            for (k, v) in headers {
                req = req.header(k, v);
            }
        }
        let resp = req.send().await?.error_for_status()?;
        Ok(resp.text().await?)
    }

    /// Write `code` to `scripts/<company>.py`, creating the directory if needed.
    pub async fn save_script(&self, company: &str, code: &str) -> Result<(), ToolError> {
        validate_company(company)?;
        let dir = self.paths.scripts_dir();
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(format!("{}.py", company)), code)?;
        Ok(())
    }

    /// Spawn the per-company script via the configured runner and parse
    /// its stdout as NDJSON. Empty stdout is legal (means: no matches).
    /// Non-zero exit becomes `ToolError::Script`; wall-time over
    /// `script_timeout` becomes `ToolError::ScriptTimeout` (the child
    /// receives SIGKILL when the future is dropped).
    pub async fn run_script(&self, company: &str) -> Result<Vec<RawJob>, ToolError> {
        validate_company(company)?;
        let path = self.paths.scripts_dir().join(format!("{}.py", company));
        let (program, args) = self.script_runner.split_first().expect(
            "script_runner is enforced non-empty in CoreTools::with_script_runner",
        );

        let fut = Command::new(program)
            .args(args)
            .arg(&path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();

        let output = match timeout(self.script_timeout, fut).await {
            Ok(r) => r?,
            Err(_) => return Err(ToolError::ScriptTimeout(self.script_timeout)),
        };

        if !output.status.success() {
            return Err(ToolError::Script {
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: output.status.code().unwrap_or(-1),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut jobs = Vec::new();
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let job: RawJob = serde_json::from_str(line)
                .map_err(|e| ToolError::InvalidNdjson(format!("{e}: {line}")))?;
            jobs.push(job);
        }
        Ok(jobs)
    }

    /// Read `memory/profile.md`. Returns `Io(NotFound)` if missing.
    pub async fn read_profile(&self) -> Result<String, ToolError> {
        let path = self.paths.memory_dir().join("profile.md");
        Ok(std::fs::read_to_string(path)?)
    }

    /// Read `memory/filters.json`. A missing file yields `Filters::default()`.
    pub async fn read_filters(&self) -> Result<Filters, ToolError> {
        let path = self.paths.memory_dir().join("filters.json");
        match std::fs::read_to_string(&path) {
            Ok(s) => Ok(serde_json::from_str(&s)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Filters::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Overwrite `memory/profile.md`, creating the directory if needed.
    pub async fn write_profile(&self, content: &str) -> Result<(), ToolError> {
        let dir = self.paths.memory_dir();
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("profile.md"), content)?;
        Ok(())
    }

    /// Overwrite `memory/filters.json` with pretty-printed JSON.
    pub async fn write_filters(&self, filters: &Filters) -> Result<(), ToolError> {
        let dir = self.paths.memory_dir();
        std::fs::create_dir_all(&dir)?;
        let s = serde_json::to_string_pretty(filters)?;
        std::fs::write(dir.join("filters.json"), s)?;
        Ok(())
    }

    /// Insert one row into the `runs` audit table.
    pub async fn record_run(
        &self,
        company: &str,
        result: RunResult,
    ) -> Result<(), ToolError> {
        sqlx::query(
            "INSERT INTO runs \
             (company_tag, started_at, finished_at, exit_code, new_job_count, stderr_tail, error) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(company)
        .bind(&result.started_at)
        .bind(&result.finished_at)
        .bind(result.exit_code)
        .bind(result.new_job_count)
        .bind(&result.stderr_tail)
        .bind(&result.error)
        .execute(&*self.db)
        .await?;
        Ok(())
    }

    /// Recent jobs for a company, newest first. Used by the agent to
    /// avoid re-suggesting roles the user has already been notified about.
    pub async fn list_known_jobs(
        &self,
        company: &str,
        limit: usize,
    ) -> Result<Vec<JobSummary>, ToolError> {
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT external_id, title, url, first_seen_at FROM jobs \
             WHERE company_tag = ? \
             ORDER BY first_seen_at DESC \
             LIMIT ?",
        )
        .bind(company)
        .bind(limit as i64)
        .fetch_all(&*self.db)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(external_id, title, url, first_seen_at)| JobSummary {
                external_id,
                title,
                url,
                first_seen_at,
            })
            .collect())
    }

    /// Insert one row into the `token_usage` audit table.
    pub async fn record_token_usage(&self, usage: TokenUsage) -> Result<(), ToolError> {
        sqlx::query(
            "INSERT INTO token_usage \
             (occurred_at, provider, model, purpose, input_tokens, output_tokens, company_tag) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&usage.occurred_at)
        .bind(&usage.provider)
        .bind(&usage.model)
        .bind(&usage.purpose)
        .bind(usage.input_tokens)
        .bind(usage.output_tokens)
        .bind(&usage.company_tag)
        .execute(&*self.db)
        .await?;
        Ok(())
    }
}

/// Reject company names that aren't safe filename identifiers — keeps
/// `save_script`/`run_script` from writing or executing files outside
/// `scripts/`. Public so CLI command handlers can reject bad input
/// before paying for an agent invocation.
pub fn validate_company(company: &str) -> Result<(), ToolError> {
    if company.is_empty()
        || !company
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(ToolError::InvalidCompany(company.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use tempfile::TempDir;

    async fn setup() -> (TempDir, CoreTools) {
        let dir = TempDir::new().unwrap();
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        let pool = Arc::new(db::open_memory().await.unwrap());
        // Tests use `python3` so they don't depend on uv being installed.
        let tools =
            CoreTools::with_script_runner(pool, paths, vec!["python3".into()]);
        (dir, tools)
    }

    #[tokio::test]
    async fn save_and_read_profile_roundtrip() {
        let (_dir, tools) = setup().await;
        tools.write_profile("# Profile\n\nHi.").await.unwrap();
        assert_eq!(tools.read_profile().await.unwrap(), "# Profile\n\nHi.");
    }

    #[tokio::test]
    async fn read_profile_missing_errors() {
        let (_dir, tools) = setup().await;
        let err = tools.read_profile().await.unwrap_err();
        assert!(matches!(err, ToolError::Io(_)));
    }

    #[tokio::test]
    async fn read_filters_missing_returns_default() {
        let (_dir, tools) = setup().await;
        let filters = tools.read_filters().await.unwrap();
        assert_eq!(filters, Filters::default());
    }

    #[tokio::test]
    async fn save_and_read_filters_roundtrip() {
        let (_dir, tools) = setup().await;
        let f = Filters {
            title_deny: vec!["Manager".into(), "Director".into()],
            location_allow_countries: vec!["US".into()],
            require_remote_or_locations: vec!["San Francisco".into()],
            clearance_deny: vec!["TS/SCI".into()],
        };
        tools.write_filters(&f).await.unwrap();
        assert_eq!(tools.read_filters().await.unwrap(), f);
    }

    #[tokio::test]
    async fn save_script_writes_under_scripts_dir() {
        let (_dir, tools) = setup().await;
        tools
            .save_script("microsoft", "print('hi')")
            .await
            .unwrap();
        let path = tools.paths().scripts_dir().join("microsoft.py");
        assert_eq!(std::fs::read_to_string(path).unwrap(), "print('hi')");
    }

    #[tokio::test]
    async fn run_script_parses_ndjson() {
        let (_dir, tools) = setup().await;
        let code = r#"
import json
print(json.dumps({"external_id": "1", "title": "SWE", "url": "https://x"}))
print(json.dumps({"external_id": "2", "title": "PM", "url": "https://y", "location": ["SF"]}))
"#;
        tools.save_script("microsoft", code).await.unwrap();
        let jobs = tools.run_script("microsoft").await.unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].external_id, "1");
        assert_eq!(jobs[1].location.as_deref(), Some(&["SF".to_string()][..]));
    }

    #[tokio::test]
    async fn run_script_surfaces_non_zero_exit() {
        let (_dir, tools) = setup().await;
        tools
            .save_script("broken", "import sys; sys.exit(7)")
            .await
            .unwrap();
        let err = tools.run_script("broken").await.unwrap_err();
        assert!(matches!(err, ToolError::Script { exit_code: 7, .. }));
    }

    #[tokio::test]
    async fn run_script_rejects_bad_ndjson() {
        let (_dir, tools) = setup().await;
        tools
            .save_script("noisy", "print('not json')")
            .await
            .unwrap();
        let err = tools.run_script("noisy").await.unwrap_err();
        assert!(matches!(err, ToolError::InvalidNdjson(_)));
    }

    #[tokio::test]
    async fn run_script_empty_stdout_is_ok() {
        let (_dir, tools) = setup().await;
        tools.save_script("quiet", "").await.unwrap();
        let jobs = tools.run_script("quiet").await.unwrap();
        assert!(jobs.is_empty());
    }

    #[tokio::test]
    async fn record_run_persists_row() {
        let (_dir, tools) = setup().await;
        tools
            .record_run(
                "microsoft",
                RunResult {
                    started_at: "2026-06-08T20:00:00Z".into(),
                    finished_at: Some("2026-06-08T20:00:05Z".into()),
                    exit_code: Some(0),
                    new_job_count: Some(3),
                    stderr_tail: None,
                    error: None,
                },
            )
            .await
            .unwrap();
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM runs WHERE company_tag = ?")
            .bind("microsoft")
            .fetch_one(tools.db())
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn list_known_jobs_returns_recent_first() {
        let (_dir, tools) = setup().await;
        sqlx::query(
            "INSERT INTO jobs (company_tag, external_id, title, url, first_seen_at) \
             VALUES ('microsoft', 'a', 'older', 'https://a', '2026-06-01T00:00:00Z')",
        )
        .execute(tools.db())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO jobs (company_tag, external_id, title, url, first_seen_at) \
             VALUES ('microsoft', 'b', 'newer', 'https://b', '2026-06-07T00:00:00Z')",
        )
        .execute(tools.db())
        .await
        .unwrap();

        let jobs = tools.list_known_jobs("microsoft", 5).await.unwrap();
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].external_id, "b");
        assert_eq!(jobs[1].external_id, "a");
    }

    #[tokio::test]
    async fn list_known_jobs_respects_limit() {
        let (_dir, tools) = setup().await;
        for i in 0..5 {
            sqlx::query(
                "INSERT INTO jobs (company_tag, external_id, title, url) \
                 VALUES ('microsoft', ?, 'T', 'https://x')",
            )
            .bind(i.to_string())
            .execute(tools.db())
            .await
            .unwrap();
        }
        let jobs = tools.list_known_jobs("microsoft", 3).await.unwrap();
        assert_eq!(jobs.len(), 3);
    }

    #[tokio::test]
    async fn save_script_rejects_unsafe_company_names() {
        let (_dir, tools) = setup().await;
        for bad in ["", "..", "../etc", "with/slash", "with space", "../../x"] {
            let err = tools.save_script(bad, "print()").await.unwrap_err();
            assert!(
                matches!(err, ToolError::InvalidCompany(_)),
                "expected InvalidCompany for {bad:?}, got {err:?}"
            );
        }
        // Sanity: a safe name still works.
        tools.save_script("micro-soft_42", "print()").await.unwrap();
    }

    #[test]
    fn validate_company_accepts_safe_names() {
        assert!(validate_company("microsoft").is_ok());
        assert!(validate_company("micro_soft").is_ok());
        assert!(validate_company("micro-soft-42").is_ok());
    }

    #[test]
    fn validate_company_rejects_bad_names() {
        for bad in ["", ".", "..", "../etc", "with/slash", "with space"] {
            assert!(validate_company(bad).is_err(), "{bad:?} should be invalid");
        }
    }

    #[tokio::test]
    async fn record_token_usage_persists_row() {
        let (_dir, tools) = setup().await;
        tools
            .record_token_usage(TokenUsage {
                occurred_at: "2026-06-08T20:00:00Z".into(),
                provider: "anthropic_api".into(),
                model: Some("claude-sonnet-4-5".into()),
                purpose: "profile_init".into(),
                input_tokens: Some(1200),
                output_tokens: Some(800),
                company_tag: None,
            })
            .await
            .unwrap();
        let (n,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM token_usage WHERE purpose = ?")
                .bind("profile_init")
                .fetch_one(tools.db())
                .await
                .unwrap();
        assert_eq!(n, 1);
    }
}
