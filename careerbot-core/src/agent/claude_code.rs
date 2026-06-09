//! Subprocess-based driver that spawns `claude -p ...` so users with a
//! Claude Code subscription can run careerbot without an Anthropic API
//! key. Claude Code runs its own agent loop; we hand it a system
//! prompt, a user prompt, and an MCP config that points back at
//! `careerbot mcp-server`. The careers tools come back to our daemon
//! over MCP stdio, the assistant's final text comes back to us as the
//! `result` field of `--output-format json`.

use super::{AgentDriver, AgentError, AgentResult, Budget, Capabilities, Cost, ToolKit};
use crate::tools::ToolError;
use crate::types::TokenUsage;
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;
use tracing::warn;

const PROVIDER: &str = "claude_code";

pub struct ClaudeCodeDriver {
    /// Path to the `claude` binary. Resolved at construction so an
    /// `agent.driver = claude_code` config error surfaces at startup
    /// rather than at first command.
    claude_bin: PathBuf,
    /// Path to *this* careerbot binary — written into the MCP config
    /// the `claude` subprocess receives so it can spawn us back as the
    /// MCP server.
    careerbot_bin: PathBuf,
}

#[derive(Debug)]
pub enum ClaudeCodeError {
    ClaudeMissing(String),
    SelfPathUnknown(String),
}

impl std::fmt::Display for ClaudeCodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClaudeMissing(reason) => {
                write!(f, "`claude` binary not usable on PATH: {reason}")
            }
            Self::SelfPathUnknown(reason) => {
                write!(f, "could not resolve current careerbot binary: {reason}")
            }
        }
    }
}

impl std::error::Error for ClaudeCodeError {}

impl ClaudeCodeDriver {
    /// Build a driver. Verifies that `claude --version` exits 0 so we
    /// fail fast if Claude Code isn't installed or isn't on PATH.
    pub fn new() -> Result<Self, ClaudeCodeError> {
        let probe = std::process::Command::new("claude")
            .arg("--version")
            .output()
            .map_err(|e| ClaudeCodeError::ClaudeMissing(e.to_string()))?;
        if !probe.status.success() {
            return Err(ClaudeCodeError::ClaudeMissing(format!(
                "claude --version exited with {:?}",
                probe.status.code()
            )));
        }

        let careerbot_bin =
            std::env::current_exe().map_err(|e| ClaudeCodeError::SelfPathUnknown(e.to_string()))?;
        Ok(Self {
            claude_bin: PathBuf::from("claude"),
            careerbot_bin,
        })
    }
}

#[async_trait]
impl AgentDriver for ClaudeCodeDriver {
    async fn run(
        &self,
        prompt: String,
        system: String,
        tools: ToolKit,
        _budget: Option<Budget>,
        purpose: &str,
    ) -> Result<AgentResult, AgentError> {
        let mcp_config = build_mcp_config(&self.careerbot_bin);
        let temp = tempfile::Builder::new()
            .prefix("careerbot-mcp-")
            .suffix(".json")
            .tempfile()
            .map_err(|e| AgentError::Tool(ToolError::Io(e)))?;
        std::fs::write(temp.path(), mcp_config.to_string())
            .map_err(|e| AgentError::Tool(ToolError::Io(e)))?;

        let output = Command::new(&self.claude_bin)
            .arg("-p")
            .arg(&prompt)
            .arg("--append-system-prompt")
            .arg(&system)
            .arg("--mcp-config")
            .arg(temp.path())
            .arg("--output-format")
            .arg("json")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| AgentError::Tool(ToolError::Io(e)))?;

        if !output.status.success() {
            let body = String::from_utf8_lossy(&output.stderr).into_owned();
            return Err(AgentError::Api {
                status: output
                    .status
                    .code()
                    .unwrap_or(-1)
                    .clamp(0, i32::from(u16::MAX)) as u16,
                body,
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: ClaudeResult = serde_json::from_str(stdout.trim())?;

        if parsed.is_error {
            return Err(AgentError::InvalidResponse(parsed.result));
        }

        record_usage_best_effort(
            &tools,
            purpose,
            parsed.usage.input_tokens,
            parsed.usage.output_tokens,
        )
        .await;

        Ok(AgentResult {
            text: parsed.result,
            // Claude Code runs its own internal loop; per-tool detail
            // would need parsing a different `--output-format`. Leave
            // empty for now.
            tool_calls: Vec::new(),
            cost: Some(Cost {
                input_tokens: parsed.usage.input_tokens,
                output_tokens: parsed.usage.output_tokens,
            }),
        })
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            // Claude Code ships its own WebSearch tool.
            native_web_search: true,
            file_attachments: true,
            rate_limit_aware: true,
            streaming: false,
        }
    }
}

/// Build the MCP config the `claude` subprocess reads. It tells claude
/// to spawn careerbot as an MCP server so the tool surface our agent
/// expects is available inside its loop.
fn build_mcp_config(careerbot_bin: &Path) -> Value {
    json!({
        "mcpServers": {
            "careerbot": {
                "command": careerbot_bin.to_string_lossy(),
                "args": ["mcp-server"]
            }
        }
    })
}

#[derive(Deserialize, Default)]
struct ClaudeResult {
    #[serde(default)]
    result: String,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    usage: ClaudeUsage,
}

#[derive(Deserialize, Default)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
}

async fn record_usage_best_effort(toolkit: &ToolKit, purpose: &str, input: u64, output: u64) {
    if input == 0 && output == 0 {
        return;
    }
    let usage = TokenUsage {
        occurred_at: chrono::Utc::now().to_rfc3339(),
        provider: PROVIDER.to_string(),
        model: None,
        purpose: purpose.to_string(),
        input_tokens: Some(input as i64),
        output_tokens: Some(output as i64),
        company_tag: None,
    };
    if let Err(e) = toolkit.core.record_token_usage(usage).await {
        warn!(?e, "failed to record token usage");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_config_points_at_supplied_binary() {
        let cfg = build_mcp_config(Path::new("/usr/local/bin/careerbot"));
        assert_eq!(
            cfg["mcpServers"]["careerbot"]["command"],
            "/usr/local/bin/careerbot"
        );
        assert_eq!(
            cfg["mcpServers"]["careerbot"]["args"],
            json!(["mcp-server"])
        );
    }

    #[test]
    fn parse_typical_claude_result() {
        let raw = r#"{"type":"result","is_error":false,"result":"Hi!","usage":{"input_tokens":6,"output_tokens":8}}"#;
        let r: ClaudeResult = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result, "Hi!");
        assert!(!r.is_error);
        assert_eq!(r.usage.input_tokens, 6);
        assert_eq!(r.usage.output_tokens, 8);
    }

    #[test]
    fn parse_tolerates_extra_fields_from_newer_claude_versions() {
        // Sample crammed with the cache_* fields current Claude Code emits.
        let raw = r#"{
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "duration_ms": 2002,
            "result": "Done.",
            "stop_reason": "end_turn",
            "total_cost_usd": 0.0173,
            "usage": {
                "input_tokens": 6,
                "cache_creation_input_tokens": 27743,
                "cache_read_input_tokens": 0,
                "output_tokens": 8,
                "iterations": []
            },
            "modelUsage": {"claude-opus-4-7": {"inputTokens": 6}}
        }"#;
        let r: ClaudeResult = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result, "Done.");
        assert_eq!(r.usage.input_tokens, 6);
        assert_eq!(r.usage.output_tokens, 8);
    }

    #[test]
    fn parse_error_result_surfaces_is_error_flag() {
        let raw =
            r#"{"is_error":true,"result":"oops","usage":{"input_tokens":1,"output_tokens":0}}"#;
        let r: ClaudeResult = serde_json::from_str(raw).unwrap();
        assert!(r.is_error);
        assert_eq!(r.result, "oops");
    }
}
