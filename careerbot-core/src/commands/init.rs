//! `careerbot init` — testable helpers. The interactive prompt flow
//! stays in the binary crate so this module is unit-testable without
//! a tty.

use super::CommandError;
use crate::paths::Paths;

/// Has careerbot been set up at this paths layout already? We treat
/// any of the user-visible artifacts (config.toml, memory/,
/// scripts/) as evidence of a prior init.
pub fn is_initialized(paths: &Paths) -> bool {
    paths.config_file().exists() || paths.memory_dir().exists() || paths.scripts_dir().exists()
}

/// Remove `data_dir` (config, memory, scripts, db.sqlite) and the
/// runtime state dir's `daemon.*` artefacts. Called by `init --force`
/// before re-running the wizard.
pub fn reset_state(paths: &Paths) -> Result<(), CommandError> {
    if paths.data_dir().exists() {
        std::fs::remove_dir_all(paths.data_dir())?;
    }
    // The state dir holds runtime files (socket, etc.) and is recreated
    // by the daemon on next start; wipe it to be tidy.
    if paths.state_dir().exists() {
        std::fs::remove_dir_all(paths.state_dir())?;
    }
    Ok(())
}

/// Best-effort detection of the `claude` binary used by the
/// `ClaudeCodeDriver`. We don't fail init when it's missing; the
/// wizard just doesn't offer it as a choice.
pub fn claude_on_path() -> bool {
    std::process::Command::new("claude")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn rooted() -> (TempDir, Paths) {
        let dir = TempDir::new().unwrap();
        let paths = Paths::rooted_at(dir.path().join("data"), dir.path().join("state"));
        (dir, paths)
    }

    #[test]
    fn empty_paths_are_not_initialised() {
        let (_dir, paths) = rooted();
        assert!(!is_initialized(&paths));
    }

    #[test]
    fn config_file_alone_counts_as_initialised() {
        let (_dir, paths) = rooted();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        std::fs::write(paths.config_file(), "").unwrap();
        assert!(is_initialized(&paths));
    }

    #[test]
    fn memory_dir_alone_counts_as_initialised() {
        let (_dir, paths) = rooted();
        std::fs::create_dir_all(paths.memory_dir()).unwrap();
        assert!(is_initialized(&paths));
    }

    #[test]
    fn reset_state_wipes_data_and_state() {
        let (_dir, paths) = rooted();
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        std::fs::write(paths.config_file(), "x = 1").unwrap();
        std::fs::create_dir_all(paths.state_dir()).unwrap();
        std::fs::write(paths.state_dir().join("daemon.pid"), "1234").unwrap();

        reset_state(&paths).unwrap();

        assert!(!paths.data_dir().exists());
        assert!(!paths.state_dir().exists());
    }

    #[test]
    fn reset_state_succeeds_on_empty_layout() {
        let (_dir, paths) = rooted();
        reset_state(&paths).unwrap();
    }
}
