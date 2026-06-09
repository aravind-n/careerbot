//! Per-company tick scheduler. Discovers companies from
//! `scripts/*.py`, spawns one task per company on its own interval
//! with jitter, runs the script + dedups against the `jobs` table +
//! records a `runs` row per tick. The `/run-now` endpoint pokes a
//! task so it ticks immediately.
//!
//! Notifications are dispatched by the *next* commit's hook; this
//! commit just gets jobs into the database.

use crate::notifications::{Notification, NotificationChannel};
use crate::runtime::Runtime;
use crate::tools::ToolError;
use crate::types::{RawJob, RunResult};
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
    channel: Arc<dyn NotificationChannel>,
}

impl Scheduler {
    /// Build the scheduler struct without spawning per-company tasks.
    /// Use [`Self::start_loops`] to spin them up; tests typically skip
    /// that step and drive ticks directly with `tick_company`.
    pub fn new(runtime: Arc<Runtime>, channel: Arc<dyn NotificationChannel>) -> Arc<Self> {
        Arc::new(Self {
            runtime,
            pokes: Mutex::new(HashMap::new()),
            poke_all: Arc::new(Notify::new()),
            channel,
        })
    }

    /// Discover companies from `scripts/*.py` and spawn one interval
    /// task per company. Listens on `shutdown` so SIGINT/SIGTERM
    /// stops them cleanly.
    pub async fn start_loops(&self, shutdown: Arc<Notify>) -> std::io::Result<()> {
        let companies = discover_companies(&self.runtime)?;
        let poll_interval = config_seconds(
            &self.runtime,
            "service.poll_interval_hours",
            DEFAULT_POLL_INTERVAL_SECONDS / 3600,
        ) * 3600;
        let startup_jitter_max = config_seconds(
            &self.runtime,
            "service.startup_jitter_seconds",
            DEFAULT_STARTUP_JITTER_SECONDS,
        );

        let mut pokes = self.pokes.lock().await;
        for company in &companies {
            let poke = Arc::new(Notify::new());
            pokes.insert(company.clone(), poke.clone());
            let startup_offset = pseudo_jitter(startup_jitter_max);
            spawn_company_loop(
                self.runtime.clone(),
                company.clone(),
                Duration::from_secs(poll_interval),
                Duration::from_secs(startup_offset),
                poke,
                self.poke_all.clone(),
                shutdown.clone(),
                self.channel.clone(),
            );
        }

        info!(
            companies = companies.len(),
            poll_interval_seconds = poll_interval,
            "scheduler started"
        );
        Ok(())
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
        perform_tick(&self.runtime, company, &self.channel).await
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
    channel: Arc<dyn NotificationChannel>,
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
            match perform_tick(&runtime, &company, &channel).await {
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

async fn perform_tick(
    runtime: &Arc<Runtime>,
    company: &str,
    channel: &Arc<dyn NotificationChannel>,
) -> Result<usize, ToolError> {
    let started_at = chrono::Utc::now().to_rfc3339();
    let outcome = runtime.tools.run_script(company).await;
    let finished_at = chrono::Utc::now().to_rfc3339();

    let (result, new_jobs) = match outcome {
        Ok(jobs) => {
            let new_jobs = runtime.tools.insert_jobs(company, &jobs).await?;
            let result = RunResult {
                started_at,
                finished_at: Some(finished_at),
                exit_code: Some(0),
                new_job_count: Some(new_jobs.len() as i64),
                stderr_tail: None,
                error: None,
            };
            (result, Ok(new_jobs))
        }
        Err(e) => {
            let (exit_code, stderr_tail) = match &e {
                ToolError::Script { stderr, exit_code } => (Some(*exit_code), Some(stderr.clone())),
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

    let new_jobs = new_jobs?;
    if !new_jobs.is_empty() {
        notify_about(runtime, company, &new_jobs, channel.as_ref()).await;
    }
    Ok(new_jobs.len())
}

async fn notify_about(
    runtime: &Arc<Runtime>,
    company: &str,
    new_jobs: &[(i64, RawJob)],
    channel: &dyn NotificationChannel,
) {
    let notification = build_notification(company, new_jobs);
    let sent_at = chrono::Utc::now().to_rfc3339();

    let (success, error) = match channel.send(notification).await {
        Ok(()) => (true, None::<String>),
        Err(e) => (false, Some(e.to_string())),
    };

    for (job_id, _) in new_jobs {
        if let Err(e) = runtime
            .tools
            .record_notification(*job_id, channel.name(), &sent_at, success, error.as_deref())
            .await
        {
            warn!(?e, "failed to record notification audit row");
        }
    }
}

fn build_notification(company: &str, new_jobs: &[(i64, RawJob)]) -> Notification {
    let display_company = title_case(company);
    match new_jobs {
        [(_, job)] => {
            let body = job
                .location
                .as_ref()
                .map(|locs| locs.join(", "))
                .unwrap_or_default();
            Notification {
                title: format!("{display_company}: {}", job.title),
                body,
                click_url: Some(job.url.clone()),
            }
        }
        many => {
            let titles: Vec<&str> = many.iter().take(3).map(|(_, j)| j.title.as_str()).collect();
            let mut body = titles.join(" · ");
            if many.len() > 3 {
                body.push_str(" · …");
            }
            Notification {
                title: format!("{display_company}: {} new matches", many.len()),
                body,
                click_url: None,
            }
        }
    }
}

fn title_case(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
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
    use crate::notifications::test_support::MockChannel;
    use crate::paths::Paths;
    use crate::tools::CoreTools;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::Mutex as AsyncMutex;

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

    /// Build a scheduler without spinning up interval tasks so tests
    /// can drive ticks deterministically.
    fn scheduler_with_mock_channel(
        rt: Arc<Runtime>,
    ) -> (Arc<Scheduler>, Arc<AsyncMutex<Vec<Notification>>>) {
        let (channel, recorded) = MockChannel::new();
        let scheduler = Scheduler::new(rt, Arc::new(channel));
        (scheduler, recorded)
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

        let (scheduler, _recorded) = scheduler_with_mock_channel(rt.clone());

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

        let (scheduler, _recorded) = scheduler_with_mock_channel(rt.clone());
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
        let (scheduler, _recorded) = scheduler_with_mock_channel(rt);
        assert!(!scheduler.poke(Some("does-not-exist")).await);
        // Global poke still succeeds even with no companies.
        assert!(scheduler.poke(None).await);
    }

    #[tokio::test]
    async fn tick_dispatches_single_job_notification() {
        let (_dir, rt) = rooted().await;
        let script = r#"
import json
print(json.dumps({"external_id": "1", "title": "Senior SWE", "url": "https://x", "location": ["Remote (US)"]}))
"#;
        rt.tools.save_script("microsoft", script).await.unwrap();

        let (scheduler, recorded) = scheduler_with_mock_channel(rt.clone());
        let new = scheduler.tick_company("microsoft").await.unwrap();
        assert_eq!(new, 1);

        let messages = recorded.lock().await;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].title, "Microsoft: Senior SWE");
        assert_eq!(messages[0].body, "Remote (US)");
        assert_eq!(messages[0].click_url.as_deref(), Some("https://x"));

        // A notifications row was written for the new job.
        let (n,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM notifications WHERE channel = 'mock' AND success = 1",
        )
        .fetch_one(rt.tools.db())
        .await
        .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn tick_batches_multi_job_notification() {
        let (_dir, rt) = rooted().await;
        let script = r#"
import json
print(json.dumps({"external_id": "1", "title": "SWE", "url": "https://a"}))
print(json.dumps({"external_id": "2", "title": "PM", "url": "https://b"}))
print(json.dumps({"external_id": "3", "title": "TPM", "url": "https://c"}))
"#;
        rt.tools.save_script("microsoft", script).await.unwrap();

        let (scheduler, recorded) = scheduler_with_mock_channel(rt.clone());
        let new = scheduler.tick_company("microsoft").await.unwrap();
        assert_eq!(new, 3);

        let messages = recorded.lock().await;
        assert_eq!(messages.len(), 1, "one batched notification, not three");
        assert!(messages[0].title.contains("3 new matches"));
        assert!(messages[0].click_url.is_none());
    }

    #[tokio::test]
    async fn tick_does_not_notify_when_no_new_jobs() {
        let (_dir, rt) = rooted().await;
        rt.tools.save_script("quiet", "").await.unwrap();

        let (scheduler, recorded) = scheduler_with_mock_channel(rt);
        let new = scheduler.tick_company("quiet").await.unwrap();
        assert_eq!(new, 0);

        assert!(recorded.lock().await.is_empty());
    }
}
