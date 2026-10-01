//! Where Topos keeps its machine-local state.
//!
//! One resolver, shared by every surface that needs it. `topos update` writes
//! its cache here, `topos install` writes its ownership ledger here, and
//! `install.sh` writes its provenance file here — three writers that had
//! drifted apart: the shell honoured `XDG_STATE_HOME` while Rust hardcoded
//! `~/.local/state`, so a user who set the variable got two directories.
//!
//! XDG *state*, not config: this is machine-local bookkeeping that nobody would
//! want synced between hosts. The spec's own rule applies — a relative
//! `XDG_STATE_HOME` is invalid and is ignored in favour of the default.

use std::path::{Path, PathBuf};

/// The user's home directory.
///
/// `USERPROFILE` is Windows' equivalent and is only consulted after `HOME`, so
/// a test harness that sets one is not overridden by an inherited other.
pub fn home_dir() -> Result<PathBuf, String> {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .find(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "cannot resolve home directory (HOME and USERPROFILE are unset)".to_string())
}

/// `%APPDATA%`, falling back to its conventional location under the profile.
/// Only reached on Windows.
pub fn app_data(home: &Path) -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| home.join("AppData/Roaming"))
}

/// Directory holding `install.json` and `update-check.json` — or
/// `%APPDATA%\topos` on Windows, which has no XDG base directories.
///
/// `topos uninstall` prunes this directory by name once every harness is
/// clear, so a cache file dropped here is removed with the rest — which is
/// what a user who uninstalled wants anyway.
pub fn state_dir(home: &Path) -> PathBuf {
    if cfg!(windows) {
        app_data(home).join("topos")
    } else {
        xdg_state()
            .unwrap_or_else(|| home.join(".local/state"))
            .join("topos")
    }
}

/// `XDG_STATE_HOME`, honored only when it names an absolute path.
fn xdg_state() -> Option<PathBuf> {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every env-var-reading assertion lives in one test on purpose: `cargo
    /// test` is threaded and `set_var` is process-global, so splitting them
    /// would make the suite race against itself.
    #[test]
    fn xdg_state_home_is_honored_and_must_be_absolute() {
        let home = Path::new("/home/someone");

        // The divergence this module exists to close: a set XDG_STATE_HOME
        // used to reach `install.sh` but not the Rust ledger.
        std::env::set_var("XDG_STATE_HOME", "/xdg/state");
        assert_eq!(xdg_state(), Some(PathBuf::from("/xdg/state")));
        if !cfg!(windows) {
            assert_eq!(state_dir(home), Path::new("/xdg/state/topos"));
        }

        // The XDG spec says a relative value is invalid, so it must fall back
        // rather than produce `cwd/relative/topos`.
        std::env::set_var("XDG_STATE_HOME", "relative/state");
        assert_eq!(xdg_state(), None);
        if !cfg!(windows) {
            assert_eq!(state_dir(home), home.join(".local/state/topos"));
        }

        std::env::remove_var("XDG_STATE_HOME");
        if !cfg!(windows) {
            assert_eq!(state_dir(home), home.join(".local/state/topos"));
        }
    }

    #[test]
    fn app_data_prefers_the_environment_and_falls_back_to_the_profile() {
        let home = Path::new("/home/someone");
        std::env::set_var("APPDATA", "/roaming");
        assert_eq!(app_data(home), Path::new("/roaming"));
        std::env::set_var("APPDATA", "");
        assert_eq!(app_data(home), home.join("AppData/Roaming"));
        std::env::remove_var("APPDATA");
    }
}
