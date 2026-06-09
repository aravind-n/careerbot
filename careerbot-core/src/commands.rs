//! Composable command implementations the CLI dispatches to. Keeping
//! these in `careerbot-core` (not the binary) lets unit tests drive a
//! [`Runtime`] directly without spawning the binary or its clap parser.
//!
//! Each function takes an opened [`Runtime`] plus its own parameters
//! and returns either a structured success type or [`CommandError`].
//! IO concerns like printing or spawning `$EDITOR` stay in the CLI
//! handler; this module just composes the runtime + agent layer.

pub mod add_company;
pub mod feedback;
pub mod filters;
pub mod init;
pub mod profile;
pub mod remove_company;

use crate::agent::AgentError;
use crate::runtime::RuntimeError;
use crate::tools::ToolError;
use std::path::Path;
use std::time::SystemTime;

/// Return the mtime of `path` if we can stat it, else `None`. Callers
/// use this to detect whether the agent touched a file during its run
/// (snapshot before / compare after) — the `exists()` shortcut treats
/// any pre-existing artefact as a successful write, which it isn't.
pub(crate) fn snapshot_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// True iff `after` indicates the file was written during the window
/// between the two snapshots — either it appeared (None → Some) or
/// its mtime advanced. Anything else (disappeared, unchanged, error
/// in either snapshot) is treated as "did not write".
pub(crate) fn wrote_during(before: Option<SystemTime>, after: Option<SystemTime>) -> bool {
    match (before, after) {
        (None, Some(_)) => true,
        (Some(b), Some(a)) => a > b,
        _ => false,
    }
}

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
            Self::Runtime(e) => write!(f, "{e}"),
            Self::Agent(e) => write!(f, "{e}"),
            Self::Tool(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::NotFound { what } => write!(f, "{what} not found"),
            Self::InvalidInput(s) => write!(f, "{s}"),
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
