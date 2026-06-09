//! `careerbot feedback "..."` — the fourth agent trigger.

use super::CommandError;
use crate::agent::{Cost, ToolKit, prompts};
use crate::runtime::Runtime;

#[derive(Debug, Clone)]
pub struct FeedbackOutput {
    /// The assistant's closing summary.
    pub text: String,
    pub tool_calls: usize,
    pub cost: Option<Cost>,
}

/// Run the `feedback` agent against the user's free-form text. The
/// agent is expected to call read_profile / read_filters and then
/// targeted write_profile / write_filters as needed; we just return
/// the cost and the summary.
pub async fn send(rt: &Runtime, text: &str) -> Result<FeedbackOutput, CommandError> {
    if text.trim().is_empty() {
        return Err(CommandError::InvalidInput("feedback is empty".into()));
    }

    let driver = rt.build_driver()?;
    let toolkit = ToolKit::in_process(rt.tools.clone());
    let result = driver
        .run(
            text.to_string(),
            prompts::FEEDBACK.to_string(),
            toolkit,
            None,
            "feedback",
        )
        .await?;

    Ok(FeedbackOutput {
        text: result.text,
        tool_calls: result.tool_calls.len(),
        cost: result.cost,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::paths::Paths;
    use serde_json::json;
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
        let rt = Runtime::open_at(paths).await.unwrap();
        (dir, rt, server)
    }

    #[tokio::test]
    async fn rejects_empty_feedback() {
        let (_dir, rt, _server) = rooted_with_mock().await;
        let err = match send(&rt, "   ").await {
            Err(e) => e,
            Ok(_) => panic!("expected InvalidInput"),
        };
        assert!(matches!(err, CommandError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn drives_agent_and_returns_summary() {
        let (_dir, rt, server) = rooted_with_mock().await;

        // Iteration 1: agent reads the profile (which doesn't exist, so the
        // tool errors — write_profile will create it).
        Mock::given(method("POST"))
            .and(wpath("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{
                    "type": "tool_use",
                    "id": "tu1",
                    "name": "write_profile",
                    "input": {"content": "# Profile\n\n## Preferences\n- Prefer: compilers\n"}
                }],
                "usage": {"input_tokens": 50, "output_tokens": 15}
            })))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;

        // Iteration 2: end turn.
        Mock::given(method("POST"))
            .and(wpath("/v1/messages"))
            .and(body_string_contains("tool_result"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "Added compiler preference."}],
                "usage": {"input_tokens": 30, "output_tokens": 5}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let output = send(&rt, "more compiler roles please").await.unwrap();
        assert_eq!(output.text, "Added compiler preference.");
        assert_eq!(output.tool_calls, 1);

        let written = rt.tools.read_profile().await.unwrap();
        assert!(written.contains("compilers"));
    }
}
