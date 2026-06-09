use std::path::{Path, PathBuf};
use std::{fmt, io};

/// In-memory view of `config.toml` with dot-key get/set/list/unset.
///
/// `Config::load` treats a missing file as an empty document, so callers can
/// freely set keys on a fresh install without touching the filesystem first.
/// `save()` writes the (possibly empty) table back via `toml::to_string`.
#[derive(Debug, Clone)]
pub struct Config {
    path: PathBuf,
    table: toml::Table,
}

#[derive(Debug)]
pub enum ConfigError {
    InvalidKey(String),
    NotATable { key: String },
    Io(io::Error),
    Parse(toml::de::Error),
    Serialize(toml::ser::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey(k) => write!(f, "invalid config key {k:?}"),
            Self::NotATable { key } => {
                write!(f, "cannot descend into {key:?}: not a table")
            }
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse(e) => write!(f, "{e}"),
            Self::Serialize(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<io::Error> for ConfigError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl Config {
    /// Load `config.toml` from `path`. A missing file yields an empty config.
    pub fn load(path: impl Into<PathBuf>) -> Result<Self, ConfigError> {
        let path = path.into();
        let table = match std::fs::read_to_string(&path) {
            Ok(s) => toml::from_str(&s).map_err(ConfigError::Parse)?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => toml::Table::new(),
            Err(e) => return Err(ConfigError::Io(e)),
        };
        Ok(Self { path, table })
    }

    /// Write the current table back to the file.
    pub fn save(&self) -> Result<(), ConfigError> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string(&self.table).map_err(ConfigError::Serialize)?;
        std::fs::write(&self.path, s)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get a value at `key` (e.g. `agent.anthropic_api.api_key`).
    pub fn get(&self, key: &str) -> Option<toml::Value> {
        let parts = split_key(key)?;
        let (last, prefix) = parts.split_last()?;
        let mut table = &self.table;
        for part in prefix {
            match table.get(*part) {
                Some(toml::Value::Table(t)) => table = t,
                _ => return None,
            }
        }
        table.get(*last).cloned()
    }

    /// Insert or overwrite the value at `key`, creating intermediate tables.
    pub fn set(&mut self, key: &str, value: toml::Value) -> Result<(), ConfigError> {
        let parts = split_key(key).ok_or_else(|| ConfigError::InvalidKey(key.to_string()))?;
        let (last, prefix) = parts
            .split_last()
            .ok_or_else(|| ConfigError::InvalidKey(key.to_string()))?;

        let mut table = &mut self.table;
        let mut walked = String::new();
        for part in prefix {
            if !walked.is_empty() {
                walked.push('.');
            }
            walked.push_str(part);

            let entry = table
                .entry((*part).to_string())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
            match entry {
                toml::Value::Table(t) => table = t,
                _ => {
                    return Err(ConfigError::NotATable {
                        key: walked.clone(),
                    });
                }
            }
        }
        table.insert((*last).to_string(), value);
        Ok(())
    }

    /// Remove the value at `key`. Empty parent tables are pruned. Returns
    /// `true` if a value was removed.
    pub fn unset(&mut self, key: &str) -> bool {
        let Some(parts) = split_key(key) else {
            return false;
        };
        remove_recursive(&mut self.table, &parts)
    }

    /// Flatten the table into `(dotted_key, value)` pairs, sorted by key.
    /// Tables are recursed into; leaf values (scalars, arrays) become entries.
    pub fn list(&self) -> Vec<(String, toml::Value)> {
        let mut out = Vec::new();
        flatten(&self.table, "", &mut out);
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

fn split_key(key: &str) -> Option<Vec<&str>> {
    if key.is_empty() {
        return None;
    }
    let parts: Vec<&str> = key.split('.').collect();
    if parts.iter().any(|p| p.is_empty()) {
        return None;
    }
    Some(parts)
}

fn remove_recursive(table: &mut toml::Table, parts: &[&str]) -> bool {
    match parts {
        [] => false,
        [last] => table.remove(*last).is_some(),
        [head, rest @ ..] => {
            let Some(toml::Value::Table(child)) = table.get_mut(*head) else {
                return false;
            };
            let removed = remove_recursive(child, rest);
            if removed && child.is_empty() {
                table.remove(*head);
            }
            removed
        }
    }
}

fn flatten(table: &toml::Table, prefix: &str, out: &mut Vec<(String, toml::Value)>) {
    for (k, v) in table {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::Table(t) => flatten(t, &key, out),
            other => out.push((key, other.clone())),
        }
    }
}

/// Parse a user-supplied CLI value. Tries TOML-literal parsing first (so
/// `7724` becomes an integer, `true` a boolean, `["os","email"]` an array);
/// falls back to a bare string for unquoted identifiers like `claude_code`.
pub fn parse_value(raw: &str) -> toml::Value {
    let wrapped = format!("_v = {raw}");
    if let Ok(parsed) = toml::from_str::<toml::Table>(&wrapped)
        && let Some(v) = parsed.get("_v")
    {
        return v.clone();
    }
    toml::Value::String(raw.to_string())
}

/// Render a value for human consumption — bare strings, otherwise TOML.
pub fn render_value(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        other => render_toml_literal(other),
    }
}

/// Render a value as a canonical TOML literal (strings are quoted).
pub fn render_toml_literal(v: &toml::Value) -> String {
    let mut t = toml::Table::new();
    t.insert("v".to_string(), v.clone());
    let s = toml::to_string(&t).unwrap_or_default();
    s.trim()
        .strip_prefix("v = ")
        .map_or_else(|| s.trim().to_string(), |x| x.trim_end().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn empty(dir: &TempDir) -> Config {
        Config::load(dir.path().join("config.toml")).expect("load empty")
    }

    #[test]
    fn load_missing_file_yields_empty() {
        let dir = TempDir::new().unwrap();
        let c = empty(&dir);
        assert!(c.list().is_empty());
    }

    #[test]
    fn set_then_get_roundtrip() {
        let dir = TempDir::new().unwrap();
        let mut c = empty(&dir);
        c.set("agent.driver", toml::Value::String("claude_code".into()))
            .unwrap();
        c.set("service.port", toml::Value::Integer(7724)).unwrap();
        assert_eq!(
            c.get("agent.driver"),
            Some(toml::Value::String("claude_code".into()))
        );
        assert_eq!(c.get("service.port"), Some(toml::Value::Integer(7724)));
    }

    #[test]
    fn set_creates_intermediate_tables() {
        let dir = TempDir::new().unwrap();
        let mut c = empty(&dir);
        c.set("a.b.c.d", toml::Value::Integer(1)).unwrap();
        assert_eq!(c.get("a.b.c.d"), Some(toml::Value::Integer(1)));
    }

    #[test]
    fn set_through_non_table_errors() {
        let dir = TempDir::new().unwrap();
        let mut c = empty(&dir);
        c.set("agent.driver", toml::Value::String("claude_code".into()))
            .unwrap();
        let err = c.set("agent.driver.binary", toml::Value::String("claude".into()));
        assert!(matches!(err, Err(ConfigError::NotATable { .. })));
    }

    #[test]
    fn get_missing_returns_none() {
        let dir = TempDir::new().unwrap();
        let c = empty(&dir);
        assert!(c.get("nothing.here").is_none());
    }

    #[test]
    fn unset_removes_and_returns_true() {
        let dir = TempDir::new().unwrap();
        let mut c = empty(&dir);
        c.set("service.port", toml::Value::Integer(7724)).unwrap();
        assert!(c.unset("service.port"));
        assert!(c.get("service.port").is_none());
    }

    #[test]
    fn unset_missing_returns_false() {
        let dir = TempDir::new().unwrap();
        let mut c = empty(&dir);
        assert!(!c.unset("not.there"));
    }

    #[test]
    fn unset_prunes_empty_parents() {
        let dir = TempDir::new().unwrap();
        let mut c = empty(&dir);
        c.set("a.b.c", toml::Value::Integer(1)).unwrap();
        c.set("a.x", toml::Value::Integer(2)).unwrap();
        assert!(c.unset("a.b.c"));
        // "a.b" was the only contents of "a.b" — should now be gone.
        let keys: Vec<String> = c.list().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["a.x"]);
    }

    #[test]
    fn invalid_keys_are_rejected() {
        let dir = TempDir::new().unwrap();
        let mut c = empty(&dir);
        assert!(matches!(
            c.set("", toml::Value::Integer(1)),
            Err(ConfigError::InvalidKey(_))
        ));
        assert!(matches!(
            c.set("a..b", toml::Value::Integer(1)),
            Err(ConfigError::InvalidKey(_))
        ));
        assert!(c.get("a..b").is_none());
        assert!(!c.unset("a..b"));
    }

    #[test]
    fn list_flattens_and_sorts() {
        let dir = TempDir::new().unwrap();
        let mut c = empty(&dir);
        c.set("service.port", toml::Value::Integer(7724)).unwrap();
        c.set("agent.driver", toml::Value::String("claude_code".into()))
            .unwrap();
        let keys: Vec<String> = c.list().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["agent.driver", "service.port"]);
    }

    #[test]
    fn save_and_reload_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = Config::load(&path).unwrap();
        c.set("service.port", toml::Value::Integer(7724)).unwrap();
        c.set("agent.driver", toml::Value::String("claude_code".into()))
            .unwrap();
        c.save().unwrap();

        let reloaded = Config::load(&path).unwrap();
        assert_eq!(
            reloaded.get("service.port"),
            Some(toml::Value::Integer(7724))
        );
        assert_eq!(
            reloaded.get("agent.driver"),
            Some(toml::Value::String("claude_code".into()))
        );
    }

    #[test]
    fn parse_value_infers_types() {
        assert_eq!(parse_value("7724"), toml::Value::Integer(7724));
        assert_eq!(parse_value("true"), toml::Value::Boolean(true));
        assert_eq!(parse_value("1.5"), toml::Value::Float(1.5));
        assert_eq!(
            parse_value("claude_code"),
            toml::Value::String("claude_code".into())
        );
        assert_eq!(
            parse_value("\"quoted\""),
            toml::Value::String("quoted".into())
        );
        match parse_value("[\"os\", \"email\"]") {
            toml::Value::Array(arr) => assert_eq!(arr.len(), 2),
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn render_value_bare_strings() {
        assert_eq!(
            render_value(&toml::Value::String("claude_code".into())),
            "claude_code"
        );
        assert_eq!(render_value(&toml::Value::Integer(7724)), "7724");
        assert_eq!(render_value(&toml::Value::Boolean(true)), "true");
    }
}
