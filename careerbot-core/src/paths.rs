use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Resolves filesystem locations for careerbot's persistent and runtime state.
///
/// Linux/macOS follow the XDG Base Directory Specification:
///   data  = `$XDG_DATA_HOME`  or `~/.local/share`, suffixed with `careerbot`
///   state = `$XDG_STATE_HOME` or `~/.local/state`, suffixed with `careerbot`
///
/// Windows uses `%LOCALAPPDATA%\careerbot\` for data and a `state\`
/// subdirectory beneath it for runtime files.
#[derive(Debug, Clone)]
pub struct Paths {
    data: PathBuf,
    state: PathBuf,
}

impl Paths {
    /// Resolve paths from the current process environment.
    pub fn from_env() -> std::io::Result<Self> {
        let (data, state) = default_roots()?;
        Ok(Self { data, state })
    }

    /// Construct a `Paths` pointed at explicit roots — for tests and overrides.
    pub fn rooted_at(data: impl Into<PathBuf>, state: impl Into<PathBuf>) -> Self {
        Self {
            data: data.into(),
            state: state.into(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    pub fn state_dir(&self) -> &Path {
        &self.state
    }

    pub fn config_file(&self) -> PathBuf {
        self.data.join("config.toml")
    }

    pub fn db_file(&self) -> PathBuf {
        self.data.join("db.sqlite")
    }

    pub fn resume_dir(&self) -> PathBuf {
        self.data.join("resume")
    }

    pub fn memory_dir(&self) -> PathBuf {
        self.data.join("memory")
    }

    pub fn scripts_dir(&self) -> PathBuf {
        self.data.join("scripts")
    }

    /// IPC socket location. Linux prefers `$XDG_RUNTIME_DIR` when set
    /// (tmpfs, cleaned up on logout); other platforms land in the
    /// state directory, which we already manage.
    pub fn socket_path(&self) -> PathBuf {
        #[cfg(target_os = "linux")]
        {
            if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR")
                && !runtime.is_empty()
            {
                return PathBuf::from(runtime).join("careerbot.sock");
            }
        }
        self.state.join("careerbot.sock")
    }
}

fn nonempty_var(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|v| !v.is_empty())
}

#[cfg(unix)]
fn default_roots() -> std::io::Result<(PathBuf, PathBuf)> {
    let home = nonempty_var("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::other("HOME not set"))?;

    let data_root = nonempty_var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));

    let state_root = nonempty_var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/state"));

    Ok((data_root.join("careerbot"), state_root.join("careerbot")))
}

#[cfg(windows)]
fn default_roots() -> std::io::Result<(PathBuf, PathBuf)> {
    let appdata = nonempty_var("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::other("LOCALAPPDATA not set"))?;

    let data = appdata.join("careerbot");
    let state = data.join("state");
    Ok((data, state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooted_at_exposes_inputs() {
        let p = Paths::rooted_at("/data", "/state");
        assert_eq!(p.data_dir(), Path::new("/data"));
        assert_eq!(p.state_dir(), Path::new("/state"));
    }

    #[test]
    fn derived_paths_live_under_data_dir() {
        let p = Paths::rooted_at("/data", "/state");
        assert_eq!(p.config_file(), Path::new("/data/config.toml"));
        assert_eq!(p.db_file(), Path::new("/data/db.sqlite"));
        assert_eq!(p.resume_dir(), Path::new("/data/resume"));
        assert_eq!(p.memory_dir(), Path::new("/data/memory"));
        assert_eq!(p.scripts_dir(), Path::new("/data/scripts"));
    }

    #[test]
    fn from_env_returns_a_careerbot_data_dir() {
        let p = Paths::from_env().expect("HOME (or LOCALAPPDATA) must be set");
        assert!(p.data_dir().ends_with("careerbot"));
    }
}
