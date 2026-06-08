//! Composable command implementations the CLI dispatches to. Keeping
//! these in `careerbot-core` (not the binary) lets unit tests drive a
//! [`Runtime`] directly without spawning the binary or its clap parser.
//!
//! Each function takes an opened [`Runtime`] plus its own parameters
//! and returns either a structured success type or [`CommandError`].
//! IO concerns like printing or spawning `$EDITOR` stay in the CLI
//! handler; this module just composes the runtime + agent layer.

pub mod add_company;
pub mod profile;

use crate::agent::AgentError;
use crate::runtime::RuntimeError;
use crate::tools::ToolError;

#[derive(Debug)]
pub enum CommandError {
    Runtime(RuntimeError),
    Agent(AgentError),
    Tool(ToolError),
    Io(std::io::Error),
    NotFound { what: String },
    InvalidInput(String),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime(e) => write!(f, "{}", e),
            Self::Agent(e) => write!(f, "{}", e),
            Self::Tool(e) => write!(f, "{}", e),
            Self::Io(e) => write!(f, "{}", e),
            Self::NotFound { what } => write!(f, "{} not found", what),
            Self::InvalidInput(s) => write!(f, "{}", s),
        }
    }
}

impl std::error::Error for CommandError {}

impl From<RuntimeError> for CommandError {
    fn from(e: RuntimeError) -> Self {
        Self::Runtime(e)
    }
}

impl From<AgentError> for CommandError {
    fn from(e: AgentError) -> Self {
        Self::Agent(e)
    }
}

impl From<ToolError> for CommandError {
    fn from(e: ToolError) -> Self {
        Self::Tool(e)
    }
}

impl From<std::io::Error> for CommandError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
