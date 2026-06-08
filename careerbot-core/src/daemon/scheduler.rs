//! Per-company tick scheduler. Discovers companies from
//! `scripts/*.py`, spawns one task per company on its own interval
//! with jitter, runs the script + dedups against the `jobs` table +
//! records a `runs` row per tick. The `/run-now` endpoint pokes a
//! task so it ticks immediately.
//!
//! Notifications are dispatched by the *next* commit's hook; this
//! commit just gets jobs into the database.

use crate::runtime::Runtime;
use crate::tools::ToolError;
use crate::types::RunResult;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{Mutex, Notify};
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 3600;
const DEFAULT_STARTUP_JITTER_SECONDS: u64 = 60;

/// Returns the list of companies the daemon currently knows about —
/// every `scripts/<name>.py` file under the data dir.
pub fn discover_companies(runtime: &Runtime) -> std::io::Result<Vec<String>> {
    let dir = runtime.paths.scripts_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("py") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            names.push(stem.to_string());
        }
    }
    names.sort();
    Ok(names)
}

pub struct Scheduler {
    runtime: Arc<Runtime>,
    /// One per-company "poke me to tick now" notify, plus a global
    /// notify covering all companies. Created at start-up; `/run-now`
    /// without a company name triggers the global one.
    pokes: Mutex<HashMap<String, Arc<Notify>>>,
    poke_all: Arc<Notify>,
}

impl Scheduler {
    pub async fn start(runtime: Arc<Runtime>, shutdown: Arc<Notify>) -> std::io::Result<Arc<Self>> {
        let companies = discover_companies(&runtime)?;
        let poll_interval = config_seconds(
            &runtime,
            "service.poll_interval_hours",
            DEFAULT_POLL_INTERVAL_SECONDS / 3600,
        ) * 3600;
        let startup_jitter_max = config_seconds(
            &runtime,
            "service.startup_jitter_seconds",
            DEFAULT_STARTUP_JITTER_SECONDS,
        );

        let poke_all = Arc::new(Notify::new());
        let mut pokes = HashMap::new();

        for company in &companies {
            let poke = Arc::new(Notify::new());
            pokes.insert(company.clone(), poke.clone());
            let startup_offset = pseudo_jitter(startup_jitter_max);
            // We deliberately don't keep the JoinHandle — tasks listen
            // for the shared `shutdown` notify and exit cleanly when
            // it fires; the runtime drop on daemon exit catches any
            // stragglers.
            spawn_company_loop(
                runtime.clone(),
                company.clone(),
                Duration::from_secs(poll_interval),
                Duration::from_secs(startup_offset),
                poke,
                poke_all.clone(),
                shutdown.clone(),
            );
        }

        info!(
            companies = companies.len(),
            poll_interval_seconds = poll_interval,
            "scheduler started"
        );

        Ok(Arc::new(Self {
            runtime,
            pokes: Mutex::new(pokes),
            poke_all,
        }))
    }

    /// Trigger an immediate tick for `company` (or every company, when
    /// `None`). Returns `false` if the company isn't known to the
    /// scheduler.
    pub async fn poke(&self, company: Option<&str>) -> bool {
        match company {
            Some(c) => {
                let guard = self.pokes.lock().await;
                if let Some(notify) = guard.get(c) {
                    notify.notify_one();
                    true
                } else {
                    false
                }
            }
            None => {
                self.poke_all.notify_waiters();
                true
            }
        }
    }

    /// Synchronously tick a single company (no scheduler involvement).
    /// Used by tests and by the future `careerbot run-now` CLI when the
    /// daemon isn't running.
    pub async fn tick_company(&self, company: &str) -> Result<usize, ToolError> {
        perform_tick(&self.runtime, company).await
    }
}

fn spawn_company_loop(
    runtime: Arc<Runtime>,
    company: String,
    interval: Duration,
    startup_offset: Duration,
    poke: Arc<Notify>,
    poke_all: Arc<Notify>,
    shutdown: Arc<Notify>,
) {
    tokio::spawn(async move {
        // Apply the per-company startup offset, but bail out early if
        // shutdown fires before the first tick.
        tokio::select! {
            _ = tokio::time::sleep(startup_offset) => {}
            _ = shutdown.notified() => return,
        }

        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        // The first `tick().await` resolves immediately — that matches
        // PLAN.md §8 "first tick after startup runs the script".
        ticker.tick().await;

        loop {
            match perform_tick(&runtime, &company).await {
                Ok(n) => {
                    info!(%company, new_jobs = n, "tick complete");
                }
                Err(e) => {
                    warn!(%company, error = %e, "tick failed");
                }
            }

            tokio::select! {
                _ = ticker.tick() => {}
                _ = poke.notified() => {}
                _ = poke_all.notified() => {}
                _ = shutdown.notified() => break,
            }
        }
    });
}

async fn perform_tick(runtime: &Arc<Runtime>, company: &str) -> Result<usize, ToolError> {
    let started_at = chrono::Utc::now().to_rfc3339();
    let outcome = runtime.tools.run_script(company).await;
    let finished_at = chrono::Utc::now().to_rfc3339();

    let (result, new_count) = match outcome {
        Ok(jobs) => {
            let new_count = runtime.tools.insert_jobs(company, &jobs).await?;
            let result = RunResult {
                started_at,
                finished_at: Some(finished_at),
                exit_code: Some(0),
                new_job_count: Some(new_count as i64),
                stderr_tail: None,
                error: None,
            };
            (result, Ok(new_count))
        }
        Err(e) => {
            let (exit_code, stderr_tail) = match &e {
                ToolError::Script { stderr, exit_code } => {
                    (Some(*exit_code), Some(stderr.clone()))
                }
                _ => (None, None),
            };
            let result = RunResult {
                started_at,
                finished_at: Some(finished_at),
                exit_code,
                new_job_count: Some(0),
                stderr_tail,
                error: Some(e.to_string()),
            };
            (result, Err(e))
        }
    };

    runtime.tools.record_run(company, result).await?;
    new_count
}

fn config_seconds(runtime: &Runtime, key: &str, default: u64) -> u64 {
    runtime
        .config
        .get(key)
        .and_then(|v| match v {
            toml::Value::Integer(i) if i >= 0 => Some(i as u64),
            _ => None,
        })
        .unwrap_or(default)
}

/// Cheap "jitter" derived from the wall clock — good enough for
/// spreading per-company startup so we don't hammer every careers
/// site in the same second.
fn pseudo_jitter(max_seconds: u64) -> u64 {
    if max_seconds == 0 {
        return 0;
    }
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 % max_seconds)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use crate::tools::CoreTools;
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn rooted() -> (TempDir, Arc<Runtime>) {
        let dir = TempDir::new().unwrap();
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        let mut rt = Runtime::open_at(paths.clone()).await.unwrap();
        let pool = rt.tools.db().clone();
        rt.tools = Arc::new(CoreTools::with_script_runner(
            Arc::new(pool),
            paths,
            vec!["python3".into()],
        ));
        (dir, Arc::new(rt))
    }

    #[tokio::test]
    async fn discover_companies_lists_python_files() {
        let (_dir, rt) = rooted().await;
        rt.tools.save_script("microsoft", "pass").await.unwrap();
        rt.tools.save_script("google", "pass").await.unwrap();
        // Non-python file should be ignored.
        std::fs::write(rt.paths.scripts_dir().join("README.md"), "ignore").unwrap();

        let mut names = discover_companies(&rt).unwrap();
        names.sort();
        assert_eq!(names, vec!["google", "microsoft"]);
    }

    #[tokio::test]
    async fn tick_company_persists_new_jobs_and_dedups() {
        let (_dir, rt) = rooted().await;
        let script = r#"
import json
print(json.dumps({"external_id": "1", "title": "SWE", "url": "https://x"}))
print(json.dumps({"external_id": "2", "title": "PM", "url": "https://y"}))
"#;
        rt.tools.save_script("microsoft", script).await.unwrap();

        let shutdown = Arc::new(Notify::new());
        let scheduler = Scheduler::start(rt.clone(), shutdown).await.unwrap();

        let new = scheduler.tick_company("microsoft").await.unwrap();
        assert_eq!(new, 2);

        let new_again = scheduler.tick_company("microsoft").await.unwrap();
        assert_eq!(new_again, 0, "duplicates should be deduped");

        // A `runs` row landed for each tick.
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM runs WHERE company_tag = ?")
            .bind("microsoft")
            .fetch_one(rt.tools.db())
            .await
            .unwrap();
        assert_eq!(n, 2);
    }

    #[tokio::test]
    async fn tick_company_records_run_with_error_when_script_breaks() {
        let (_dir, rt) = rooted().await;
        rt.tools
            .save_script("broken", "import sys; sys.exit(3)")
            .await
            .unwrap();

        let shutdown = Arc::new(Notify::new());
        let scheduler = Scheduler::start(rt.clone(), shutdown).await.unwrap();
        let err = scheduler.tick_company("broken").await.unwrap_err();
        assert!(matches!(err, ToolError::Script { exit_code: 3, .. }));

        let (exit_code, error): (Option<i64>, Option<String>) =
            sqlx::query_as("SELECT exit_code, error FROM runs WHERE company_tag = ?")
                .bind("broken")
                .fetch_one(rt.tools.db())
                .await
                .unwrap();
        assert_eq!(exit_code, Some(3));
        assert!(error.unwrap().contains("script exited 3"));
    }

    #[tokio::test]
    async fn poke_unknown_company_returns_false() {
        let (_dir, rt) = rooted().await;
        let shutdown = Arc::new(Notify::new());
        let scheduler = Scheduler::start(rt.clone(), shutdown).await.unwrap();
        assert!(!scheduler.poke(Some("does-not-exist")).await);
        // Global poke still succeeds even with no companies.
        assert!(scheduler.poke(None).await);
    }
}
