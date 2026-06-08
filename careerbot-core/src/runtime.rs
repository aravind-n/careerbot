//! `Runtime` is the loaded-session object the CLI and daemon build
//! once at startup: it owns the parsed `config.toml`, the resolved
//! [`Paths`], an opened SQLite pool (migrations already applied), and
//! a constructed [`CoreTools`]. `build_driver` reads the
//! agent-related keys from config and returns a boxed
//! [`AgentDriver`].
//!
//! Keeping this in `careerbot-core` rather than the binary lets unit
//! tests drive the agent commands without spawning the CLI.

use crate::agent::AgentDriver;
use crate::agent::anthropic_api::AnthropicApiDriver;
use crate::config::{Config, ConfigError};
use crate::db;
use crate::paths::Paths;
use crate::tools::CoreTools;
use std::sync::Arc;

pub struct Runtime {
    pub config: Config,
    pub paths: Paths,
    pub tools: Arc<CoreTools>,
}

#[derive(Debug)]
pub enum RuntimeError {
    Io(std::io::Error),
    Config(ConfigError),
    Db(sqlx::Error),
    MissingConfig { key: &'static str, hint: &'static str },
    UnsupportedDriver(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{}", e),
            Self::Config(e) => write!(f, "{}", e),
            Self::Db(e) => write!(f, "{}", e),
            Self::MissingConfig { key, hint } => {
                write!(f, "missing config key {:?}; {}", key, hint)
            }
            Self::UnsupportedDriver(s) => write!(f, "unsupported agent.driver: {:?}", s),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<std::io::Error> for RuntimeError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ConfigError> for RuntimeError {
    fn from(e: ConfigError) -> Self {
        Self::Config(e)
    }
}

impl From<sqlx::Error> for RuntimeError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

impl Runtime {
    /// Resolve paths from the process environment, then defer to
    /// [`Self::open_at`].
    pub async fn open() -> Result<Self, RuntimeError> {
        let paths = Paths::from_env()?;
        Self::open_at(paths).await
    }

    /// Explicit-roots variant — used by tests and by callers who need
    /// to point at a non-default data directory.
    pub async fn open_at(paths: Paths) -> Result<Self, RuntimeError> {
        let config = Config::load(paths.config_file())?;
        let pool = Arc::new(db::open(&paths.db_file()).await?);
        let tools = Arc::new(CoreTools::new(pool, paths.clone()));
        Ok(Self {
            config,
            paths,
            tools,
        })
    }

    /// Build the agent driver from configuration. Currently only
    /// `anthropic_api` is implemented; setting `agent.driver` to
    /// anything else returns [`RuntimeError::UnsupportedDriver`].
    pub fn build_driver(&self) -> Result<Box<dyn AgentDriver>, RuntimeError> {
        let driver_name =
            string_config(&self.config, "agent.driver").ok_or(RuntimeError::MissingConfig {
                key: "agent.driver",
                hint:
                    "run `careerbot config agent.driver anthropic_api` (only \
                     anthropic_api is implemented in this phase)",
            })?;

        match driver_name.as_str() {
            "anthropic_api" => {
                let api_key = string_config(&self.config, "agent.anthropic_api.api_key")
                    .ok_or(RuntimeError::MissingConfig {
                        key: "agent.anthropic_api.api_key",
                        hint: "run `careerbot config agent.anthropic_api.api_key <KEY>`",
                    })?;

                let mut driver = AnthropicApiDriver::new(api_key);
                if let Some(model) = string_config(&self.config, "agent.anthropic_api.model") {
                    driver = driver.with_model(model);
                }
                if let Some(base_url) =
                    string_config(&self.config, "agent.anthropic_api.base_url")
                {
                    driver = driver.with_base_url(base_url);
                }
                Ok(Box::new(driver))
            }
            other => Err(RuntimeError::UnsupportedDriver(other.to_string())),
        }
    }
}

fn string_config(cfg: &Config, key: &str) -> Option<String> {
    cfg.get(key).and_then(|v| match v {
        toml::Value::String(s) => Some(s),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn rooted(dir: &TempDir) -> Runtime {
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        Runtime::open_at(paths).await.expect("open runtime")
    }

    #[tokio::test]
    async fn open_creates_data_dir_and_db() {
        let dir = TempDir::new().unwrap();
        let rt = rooted(&dir).await;
        assert!(rt.paths.data_dir().exists());
        assert!(rt.paths.db_file().exists());
    }

    fn expect_err(r: Result<Box<dyn AgentDriver>, RuntimeError>) -> RuntimeError {
        match r {
            Err(e) => e,
            Ok(_) => panic!("expected error, got driver"),
        }
    }

    #[tokio::test]
    async fn build_driver_errors_without_driver_key() {
        let dir = TempDir::new().unwrap();
        let rt = rooted(&dir).await;
        let err = expect_err(rt.build_driver());
        assert!(matches!(
            err,
            RuntimeError::MissingConfig {
                key: "agent.driver",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn build_driver_errors_without_api_key() {
        let dir = TempDir::new().unwrap();
        let mut rt = rooted(&dir).await;
        rt.config
            .set("agent.driver", toml::Value::String("anthropic_api".into()))
            .unwrap();
        let err = expect_err(rt.build_driver());
        assert!(matches!(
            err,
            RuntimeError::MissingConfig {
                key: "agent.anthropic_api.api_key",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn build_driver_rejects_unknown_driver() {
        let dir = TempDir::new().unwrap();
        let mut rt = rooted(&dir).await;
        rt.config
            .set("agent.driver", toml::Value::String("nonexistent".into()))
            .unwrap();
        let err = expect_err(rt.build_driver());
        assert!(matches!(err, RuntimeError::UnsupportedDriver(s) if s == "nonexistent"));
    }

    #[tokio::test]
    async fn build_driver_returns_anthropic_when_configured() {
        let dir = TempDir::new().unwrap();
        let mut rt = rooted(&dir).await;
        rt.config
            .set("agent.driver", toml::Value::String("anthropic_api".into()))
            .unwrap();
        rt.config
            .set(
                "agent.anthropic_api.api_key",
                toml::Value::String("sk-test".into()),
            )
            .unwrap();
        let driver = rt.build_driver().expect("build driver");
        // Capability flags should match AnthropicApiDriver's profile.
        let caps = driver.capabilities();
        assert!(!caps.native_web_search);
        assert!(caps.file_attachments);
    }
}
