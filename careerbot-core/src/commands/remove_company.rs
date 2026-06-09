//! `careerbot remove-company` command — deletes a company's
//! collector script and the `jobs` rows it produced. Related
//! `notifications` rows go with the jobs via ON DELETE CASCADE.

use super::CommandError;
use crate::runtime::Runtime;
use crate::tools;

#[derive(Debug, Clone)]
pub struct RemoveOutput {
    pub script_removed: bool,
    pub jobs_removed: usize,
}

pub async fn remove(rt: &Runtime, name: &str) -> Result<RemoveOutput, CommandError> {
    tools::validate_company(name).map_err(|e| CommandError::InvalidInput(e.to_string()))?;

    let script_path = rt.paths.scripts_dir().join(format!("{name}.py"));
    let script_removed = if script_path.exists() {
        std::fs::remove_file(&script_path)?;
        true
    } else {
        false
    };

    let jobs_removed = rt.tools.delete_company_jobs(name).await?;

    Ok(RemoveOutput {
        script_removed,
        jobs_removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use tempfile::TempDir;

    async fn rooted() -> (TempDir, Runtime) {
        let dir = TempDir::new().unwrap();
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        let rt = Runtime::open_at(paths).await.unwrap();
        (dir, rt)
    }

    #[tokio::test]
    async fn removes_script_and_jobs() {
        let (_dir, rt) = rooted().await;
        rt.tools.save_script("microsoft", "pass").await.unwrap();
        sqlx::query(
            "INSERT INTO jobs (company_tag, external_id, title, url) \
             VALUES ('microsoft', '1', 'SWE', 'https://x')",
        )
        .execute(rt.tools.db())
        .await
        .unwrap();

        let out = remove(&rt, "microsoft").await.unwrap();
        assert!(out.script_removed);
        assert_eq!(out.jobs_removed, 1);
        assert!(!rt.paths.scripts_dir().join("microsoft.py").exists());
    }

    #[tokio::test]
    async fn no_script_still_succeeds() {
        let (_dir, rt) = rooted().await;
        let out = remove(&rt, "phantom").await.unwrap();
        assert!(!out.script_removed);
        assert_eq!(out.jobs_removed, 0);
    }

    #[tokio::test]
    async fn rejects_unsafe_company_name() {
        let (_dir, rt) = rooted().await;
        let Err(err) = remove(&rt, "../etc").await else {
            panic!("expected InvalidInput");
        };
        assert!(matches!(err, CommandError::InvalidInput(_)));
    }
}
