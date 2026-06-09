//! `careerbot add-company` command.

use super::CommandError;
use crate::agent::{Cost, ToolKit, prompts};
use crate::runtime::Runtime;
use crate::tools;

/// Result of an `add-company` run.
#[derive(Debug, Clone)]
pub struct AddCompanyOutput {
    /// The assistant's closing text (typically a short summary).
    pub text: String,
    pub tool_calls: usize,
    pub cost: Option<Cost>,
    /// Did save-time verification (`run_script`) succeed?
    pub verified: bool,
    /// Job count returned by the verification run when `verified` is true.
    pub initial_jobs: usize,
    /// Error message from the failed verification run when `verified`
    /// is false. The script is left in place so the user can inspect.
    pub verification_error: Option<String>,
}

/// Run the `script_gen` agent for `name` (optionally hinting `url`) and
/// then perform save-time verification by executing the freshly
/// generated script through `run_script`.
///
/// The verification *result* is reported through the return value —
/// not through `Err` — because a verification failure is interesting
/// to the user (the script exists, it just didn't work yet) and the
/// CLI handler turns that into a non-zero exit on its own terms.
pub async fn add_company(
    rt: &Runtime,
    name: &str,
    url: Option<&str>,
) -> Result<AddCompanyOutput, CommandError> {
    tools::validate_company(name).map_err(|e| CommandError::InvalidInput(e.to_string()))?;

    let driver = rt.build_driver()?;
    let toolkit = ToolKit::in_process(rt.tools.clone());

    let prompt = match url {
        Some(u) => format!("Company: {name}\nCareers URL: {u}"),
        None => format!("Company: {name}\nNo URL provided — discover one."),
    };

    let result = driver
        .run(
            prompt,
            prompts::SCRIPT_GEN.to_string(),
            toolkit,
            None,
            "script_gen",
        )
        .await?;

    // Save-time verification. The script may already have been run by
    // the agent inside the loop; running it again here is the contract
    // we enforce regardless of what the agent did.
    let (verified, initial_jobs, verification_error) = match rt.tools.run_script(name).await {
        Ok(jobs) => (true, jobs.len(), None),
        Err(e) => (false, 0, Some(e.to_string())),
    };

    Ok(AddCompanyOutput {
        text: result.text,
        tool_calls: result.tool_calls.len(),
        cost: result.cost,
        verified,
        initial_jobs,
        verification_error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::paths::Paths;
    use crate::tools::CoreTools;
    use serde_json::json;
    use std::sync::Arc;
    use tempfile::TempDir;
    use wiremock::matchers::{body_string_contains, method, path as wpath};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn rooted_with_mock() -> (TempDir, Runtime, MockServer) {
        let server = MockServer::start().await;
        let dir = TempDir::new().unwrap();
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));

        let mut cfg = Config::load(paths.config_file()).unwrap();
        cfg.set("agent.driver", toml::Value::String("anthropic_api".into()))
            .unwrap();
        cfg.set(
            "agent.anthropic_api.api_key",
            toml::Value::String("test-key".into()),
        )
        .unwrap();
        cfg.set(
            "agent.anthropic_api.base_url",
            toml::Value::String(server.uri()),
        )
        .unwrap();
        cfg.save().unwrap();

        // Open the runtime, then swap in a CoreTools whose script
        // runner is `python3` — uv may not be on every dev box.
        let mut rt = Runtime::open_at(paths.clone()).await.unwrap();
        let pool = rt.tools.db().clone();
        rt.tools = Arc::new(CoreTools::with_script_runner(
            Arc::new(pool),
            paths,
            vec!["python3".into()],
        ));
        (dir, rt, server)
    }

    #[tokio::test]
    async fn add_company_rejects_unsafe_name() {
        let (_dir, rt, _server) = rooted_with_mock().await;
        let Err(err) = add_company(&rt, "../etc/passwd", None).await else {
            panic!("expected InvalidInput");
        };
        assert!(matches!(err, CommandError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn add_company_happy_path_saves_and_verifies() {
        let (_dir, rt, server) = rooted_with_mock().await;

        // Iteration 1: agent saves a working script (prints nothing,
        // which is a legitimate "zero matches" outcome).
        let script_body = "import sys\n";
        Mock::given(method("POST"))
            .and(wpath("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{
                    "type": "tool_use",
                    "id": "tu1",
                    "name": "save_script",
                    "input": {"company": "microsoft", "code": script_body}
                }],
                "usage": {"input_tokens": 100, "output_tokens": 20}
            })))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;

        // Iteration 2: end_turn.
        Mock::given(method("POST"))
            .and(wpath("/v1/messages"))
            .and(body_string_contains("tool_result"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "Saved microsoft collector."}],
                "usage": {"input_tokens": 50, "output_tokens": 5}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let output = add_company(&rt, "microsoft", None).await.unwrap();

        assert_eq!(output.text, "Saved microsoft collector.");
        assert!(output.verified, "save-time verification should pass");
        assert_eq!(output.initial_jobs, 0);
        assert!(output.verification_error.is_none());

        // The script file is on disk.
        let written = std::fs::read_to_string(rt.paths.scripts_dir().join("microsoft.py")).unwrap();
        assert_eq!(written, script_body);
    }

    #[tokio::test]
    async fn add_company_surfaces_verification_failure() {
        let (_dir, rt, server) = rooted_with_mock().await;

        // Agent saves a script that exits 1.
        let broken_script = "import sys; sys.exit(1)\n";
        Mock::given(method("POST"))
            .and(wpath("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{
                    "type": "tool_use",
                    "id": "tu1",
                    "name": "save_script",
                    "input": {"company": "broken", "code": broken_script}
                }],
                "usage": {"input_tokens": 100, "output_tokens": 20}
            })))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(wpath("/v1/messages"))
            .and(body_string_contains("tool_result"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "Saved (untested by me)."}],
                "usage": {"input_tokens": 50, "output_tokens": 5}
            })))
            .mount(&server)
            .await;

        let output = add_company(&rt, "broken", None).await.unwrap();

        assert!(!output.verified, "broken script should fail verification");
        assert!(output.verification_error.is_some());
        // The script file is still on disk so the user can inspect it.
        assert!(rt.paths.scripts_dir().join("broken.py").exists());
    }
}
