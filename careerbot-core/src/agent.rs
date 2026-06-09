//! Agent loop abstractions. The [`AgentDriver`] trait is the seam between
//! the deterministic daemon and whatever LLM harness the user has
//! credentials for; concrete implementations live in submodules.
//!
//! See PLAN.md §4 for the trait shape and §5 for the tool layer the
//! drivers share.

pub mod anthropic_api;
pub mod claude_code;
pub mod prompts;
pub mod tool_dispatch;

use crate::tools::{CoreTools, ToolError};
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

/// What the daemon (or CLI) hands to a driver invocation. `core` carries
/// the in-process tool bag for HTTP-style drivers; `mcp_config_path`
/// points at a temp file for subprocess drivers (Claude Code, Codex…)
/// and is `None` for in-process callers.
#[derive(Clone)]
pub struct ToolKit {
    pub core: Arc<CoreTools>,
    pub mcp_config_path: Option<PathBuf>,
}

impl ToolKit {
    pub fn in_process(core: Arc<CoreTools>) -> Self {
        Self {
            core,
            mcp_config_path: None,
        }
    }
}

/// Capability flags a driver advertises so the daemon can fall back to
/// emulation (or refuse a request) when a feature is missing.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools, reason = "named capability flags")]
pub struct Capabilities {
    pub native_web_search: bool,
    pub file_attachments: bool,
    pub rate_limit_aware: bool,
    pub streaming: bool,
}

/// Caller-imposed limits on a single agent invocation.
#[derive(Debug, Clone)]
pub struct Budget {
    /// Maximum tool-use cycles. Prevents runaway loops.
    pub max_iterations: u32,
    /// Maximum tokens to allow on the final `max_tokens` parameter per
    /// request to the provider.
    pub max_output_tokens: u32,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            max_output_tokens: 4096,
        }
    }
}

/// Final outcome of a driver invocation.
#[derive(Debug, Clone, Default)]
pub struct AgentResult {
    /// The assistant's last text block (the "answer").
    pub text: String,
    pub tool_calls: Vec<ToolCallSummary>,
    pub cost: Option<Cost>,
}

#[derive(Debug, Clone)]
pub struct ToolCallSummary {
    pub tool: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Cost {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug)]
pub enum AgentError {
    Http(reqwest::Error),
    Api { status: u16, body: String },
    Tool(ToolError),
    Serde(serde_json::Error),
    LoopExhausted { iterations: u32 },
    InvalidResponse(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "{e}"),
            Self::Api { status, body } => write!(f, "API error {status}: {body}"),
            Self::Tool(e) => write!(f, "tool: {e}"),
            Self::Serde(e) => write!(f, "{e}"),
            Self::LoopExhausted { iterations } => {
                write!(f, "agent loop exhausted after {iterations} iterations")
            }
            Self::InvalidResponse(s) => write!(f, "invalid driver response: {s}"),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<reqwest::Error> for AgentError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

impl From<serde_json::Error> for AgentError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e)
    }
}

impl From<ToolError> for AgentError {
    fn from(e: ToolError) -> Self {
        Self::Tool(e)
    }
}

/// A file the caller wants the agent to see alongside the prompt. The
/// driver knows how to deliver it: the Anthropic driver base64-encodes
/// the bytes and sends a `document` content block, the Claude Code
/// driver passes `--add-dir` and lets claude open the path itself.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub path: PathBuf,
    pub kind: AttachmentKind,
}

#[derive(Debug, Clone, Copy)]
pub enum AttachmentKind {
    Pdf,
}

impl AttachmentKind {
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Pdf => "application/pdf",
        }
    }
}

/// The seam between the daemon and the LLM harness.
#[async_trait]
pub trait AgentDriver: Send + Sync {
    async fn run(
        &self,
        prompt: String,
        system: String,
        tools: ToolKit,
        budget: Option<Budget>,
        purpose: &str,
        attachments: &[Attachment],
    ) -> Result<AgentResult, AgentError>;

    fn capabilities(&self) -> Capabilities;
}
