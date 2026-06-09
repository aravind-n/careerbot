//! HTTP-only driver against Anthropic's `/v1/messages`. In-process tool
//! dispatch — no MCP transport, no subprocess.
//!
//! The loop is straightforward: post the message history, walk every
//! content block in the response (preserving order so `text` and
//! `tool_use` blocks echo back to the next turn correctly), dispatch the
//! `tool_use` blocks against the in-process [`CoreTools`], and feed the
//! results back as the next user turn. When a response contains no
//! `tool_use` blocks the loop ends.

use super::tool_dispatch::{all_tools, dispatch_tool, to_anthropic_tools};
use super::{
    AgentDriver, AgentError, AgentResult, Budget, Capabilities, Cost, ToolCallSummary, ToolKit,
};
use crate::tools::default_http_client;
use crate::types::TokenUsage;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const DEFAULT_MODEL: &str = "claude-sonnet-4-5";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const PROVIDER: &str = "anthropic_api";

pub struct AnthropicApiDriver {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl AnthropicApiDriver {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: default_http_client(),
            api_key: api_key.into(),
            model: DEFAULT_MODEL.to_string(),
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }
}

#[async_trait]
impl AgentDriver for AnthropicApiDriver {
    async fn run(
        &self,
        prompt: String,
        system: String,
        tools: ToolKit,
        budget: Option<Budget>,
        purpose: &str,
    ) -> Result<AgentResult, AgentError> {
        let budget = budget.unwrap_or_default();
        let tool_defs = to_anthropic_tools(&all_tools());
        let mut messages: Vec<Value> = vec![json!({"role": "user", "content": prompt})];

        let mut total_input: u64 = 0;
        let mut total_output: u64 = 0;
        let mut tool_calls: Vec<ToolCallSummary> = Vec::new();
        let mut last_text = String::new();

        for _ in 0..budget.max_iterations {
            let body = json!({
                "model": self.model,
                "max_tokens": budget.max_output_tokens,
                "system": system,
                "messages": messages,
                "tools": tool_defs,
            });

            let resp = self
                .client
                .post(format!("{}/v1/messages", self.base_url))
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await?;

            let status = resp.status();
            if !status.is_success() {
                let body_text = resp.text().await.unwrap_or_default();
                record_usage_best_effort(
                    &tools,
                    &self.model,
                    purpose,
                    total_input,
                    total_output,
                )
                .await;
                return Err(AgentError::Api {
                    status: status.as_u16(),
                    body: body_text,
                });
            }

            let response: MessagesResponse = resp.json().await?;
            total_input = total_input.saturating_add(response.usage.input_tokens);
            total_output = total_output.saturating_add(response.usage.output_tokens);

            let mut tool_results: Vec<Value> = Vec::new();
            for block in &response.content {
                match block {
                    ContentBlock::Text { text } => {
                        last_text = text.clone();
                    }
                    ContentBlock::ToolUse { id, name, input } => {
                        let (content, is_error) = match dispatch_tool(&tools, name, input).await {
                            Ok(s) => (s, false),
                            Err(e) => (e, true),
                        };
                        tool_calls.push(ToolCallSummary {
                            tool: name.clone(),
                            success: !is_error,
                            error: if is_error { Some(content.clone()) } else { None },
                        });
                        tool_results.push(json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": content,
                            "is_error": is_error,
                        }));
                    }
                }
            }

            // Echo the assistant turn back so the next iteration has the
            // full context the model expects.
            messages.push(json!({
                "role": "assistant",
                "content": response.content,
            }));

            if tool_results.is_empty() {
                record_usage_best_effort(
                    &tools,
                    &self.model,
                    purpose,
                    total_input,
                    total_output,
                )
                .await;
                return Ok(AgentResult {
                    text: last_text,
                    tool_calls,
                    cost: Some(Cost {
                        input_tokens: total_input,
                        output_tokens: total_output,
                    }),
                });
            }

            messages.push(json!({
                "role": "user",
                "content": tool_results,
            }));
        }

        record_usage_best_effort(&tools, &self.model, purpose, total_input, total_output).await;
        Err(AgentError::LoopExhausted {
            iterations: budget.max_iterations,
        })
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            native_web_search: false,
            file_attachments: true,
            rate_limit_aware: true,
            streaming: false,
        }
    }
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
    #[serde(default)]
    usage: Usage,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

async fn record_usage_best_effort(
    toolkit: &ToolKit,
    model: &str,
    purpose: &str,
    input: u64,
    output: u64,
) {
    if input == 0 && output == 0 {
        return;
    }
    let usage = TokenUsage {
        occurred_at: chrono::Utc::now().to_rfc3339(),
        provider: PROVIDER.to_string(),
        model: Some(model.to_string()),
        purpose: purpose.to_string(),
        input_tokens: Some(input as i64),
        output_tokens: Some(output as i64),
        company_tag: None,
    };
    let _ = toolkit.core.record_token_usage(usage).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::paths::Paths;
    use crate::tools::CoreTools;
    use std::sync::Arc;
    use tempfile::TempDir;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn toolkit() -> (TempDir, ToolKit) {
        let dir = TempDir::new().unwrap();
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        let pool = Arc::new(db::open_memory().await.unwrap());
        let tools =
            CoreTools::with_script_runner(pool, paths, vec!["python3".into()]);
        (dir, ToolKit::in_process(Arc::new(tools)))
    }

    fn driver(server: &MockServer) -> AnthropicApiDriver {
        AnthropicApiDriver::new("test-key").with_base_url(server.uri())
    }

    #[tokio::test]
    async fn single_turn_text_only_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "hello"}],
                "usage": {"input_tokens": 5, "output_tokens": 3}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let (_dir, kit) = toolkit().await;
        let result = driver(&server)
            .run(
                "hi".into(),
                "be terse".into(),
                kit.clone(),
                None,
                "test",
            )
            .await
            .expect("ok");
        assert_eq!(result.text, "hello");
        assert!(result.tool_calls.is_empty());
        assert_eq!(result.cost.unwrap().input_tokens, 5);

        // record_token_usage row was written.
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM token_usage")
            .fetch_one(kit.core.db())
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn multi_turn_with_tool_use() {
        let server = MockServer::start().await;

        // Iteration 1 (up_to_n_times(1) so the mock exhausts after one
        // call and iteration 2 falls through to the next mock).
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [
                    {"type": "text", "text": "checking"},
                    {"type": "tool_use", "id": "tu1", "name": "write_profile",
                     "input": {"content": "# Profile\n\nFresh."}}
                ],
                "usage": {"input_tokens": 100, "output_tokens": 20}
            })))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;

        // Iteration 2: after tool_result echoes back, end turn.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_string_contains("tool_result"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "done"}],
                "usage": {"input_tokens": 150, "output_tokens": 5}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let (_dir, kit) = toolkit().await;
        let result = driver(&server)
            .run(
                "prompt-text".into(),
                "system".into(),
                kit.clone(),
                None,
                "test",
            )
            .await
            .expect("ok");

        assert_eq!(result.text, "done");
        assert_eq!(result.tool_calls.len(), 1);
        assert_eq!(result.tool_calls[0].tool, "write_profile");
        assert!(result.tool_calls[0].success);
        let cost = result.cost.unwrap();
        assert_eq!(cost.input_tokens, 250);
        assert_eq!(cost.output_tokens, 25);

        // write_profile actually ran.
        let written = kit.core.read_profile().await.unwrap();
        assert!(written.contains("Fresh."));
    }

    #[tokio::test]
    async fn api_error_surfaces() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .expect(1)
            .mount(&server)
            .await;

        let (_dir, kit) = toolkit().await;
        let err = driver(&server)
            .run(
                "p".into(),
                "s".into(),
                kit,
                None,
                "test",
            )
            .await
            .unwrap_err();
        match err {
            AgentError::Api { status, body } => {
                assert_eq!(status, 500);
                assert_eq!(body, "boom");
            }
            other => panic!("expected Api error, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn loop_exhausted_after_max_iterations() {
        let server = MockServer::start().await;
        // Every response asks for another read_profile — the driver never
        // gets a no-tool response, so it must give up after max_iterations.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "tool_use", "id": "tu1",
                             "name": "read_profile", "input": {}}],
                "usage": {"input_tokens": 1, "output_tokens": 1}
            })))
            .expect(3)
            .mount(&server)
            .await;

        let (_dir, kit) = toolkit().await;
        kit.core.write_profile("# stub").await.unwrap();

        let budget = Budget {
            max_iterations: 3,
            max_output_tokens: 1024,
        };
        let err = driver(&server)
            .run(
                "p".into(),
                "s".into(),
                kit,
                Some(budget),
                "test",
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AgentError::LoopExhausted { iterations: 3 }));
    }

    #[tokio::test]
    async fn unknown_tool_reports_error_to_model() {
        let server = MockServer::start().await;

        // Iteration 1: invoke an unknown tool.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "tool_use", "id": "tu1",
                             "name": "does_not_exist", "input": {}}],
                "usage": {"input_tokens": 10, "output_tokens": 2}
            })))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;

        // Iteration 2: end turn after the is_error tool_result.
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(body_string_contains("\"is_error\":true"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "content": [{"type": "text", "text": "oops"}],
                "usage": {"input_tokens": 5, "output_tokens": 1}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let (_dir, kit) = toolkit().await;
        let result = driver(&server)
            .run(
                "prompt-text".into(),
                "s".into(),
                kit,
                None,
                "test",
            )
            .await
            .expect("ok");
        assert_eq!(result.text, "oops");
        assert_eq!(result.tool_calls.len(), 1);
        assert!(!result.tool_calls[0].success);
    }
}
