//! Per-OS config locations for every supported harness.
//!
//! Each function takes `home` explicitly instead of reading `$HOME` itself, so
//! the end-to-end suite can drive the real binary against a scratch home
//! directory. `%APPDATA%` is the one environment variable consulted here:
//! Windows has no derivation of it from the profile directory that holds in
//! every deployment.

use std::path::{Path, PathBuf};

pub(crate) fn home_dir() -> Result<PathBuf, String> {
    ["HOME", "USERPROFILE"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .find(|path| !path.as_os_str().is_empty())
        .ok_or_else(|| "cannot resolve home directory (HOME and USERPROFILE are unset)".to_string())
}

/// `%APPDATA%`, falling back to its conventional location under the profile.
/// Only reached on Windows.
pub(crate) fn app_data(home: &Path) -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| home.join("AppData/Roaming"))
}

pub(crate) fn claude_config(home: &Path) -> PathBuf {
    // Not `~/.claude/settings.json` — that file is hooks/permissions only.
    // User-scope MCP servers live here, matching what `claude mcp add` writes.
    home.join(".claude.json")
}

pub(crate) fn codex_config(home: &Path) -> PathBuf {
    home.join(".codex/config.toml")
}

pub(crate) fn gemini_config(home: &Path) -> PathBuf {
    home.join(".gemini/settings.json")
}

pub(crate) fn copilot_config(home: &Path) -> PathBuf {
    home.join(".copilot/mcp-config.json")
}

pub(crate) fn cursor_config(home: &Path) -> PathBuf {
    home.join(".cursor/mcp.json")
}

/// The only file Antigravity reads for global MCP servers, for both the `agy`
/// CLI and the IDE. Its own migration moved MCP config out of the
/// per-executable data directories into here, leaving back-compat symlinks
/// behind — `~/.gemini/antigravity/mcp_config.json` is a symlink to this file,
/// so writing there would sever the link and guarantee the entry is ignored.
pub(crate) fn antigravity_config(home: &Path) -> PathBuf {
    home.join(".gemini/config/mcp_config.json")
}

/// The pi-scoped MCP override, **not** `~/.pi/agent/settings.json`.
///
/// `settings.json` is pi's own settings file — theme, provider, transport —
/// with a fixed documented key set that has no `mcpServers` in it; an entry
/// written there is read by nothing. MCP servers are resolved by pi's adapter
/// extension, and of the six locations it searches this is the only one that
/// is pi's alone (`~/.config/mcp/mcp.json` and `~/.agents/mcp.json` are shared
/// across tools, and `topos install pi` has no business owning those).
pub(crate) fn pi_config(home: &Path) -> PathBuf {
    home.join(".pi/agent/mcp.json")
}

pub(crate) fn claude_desktop_config(home: &Path) -> PathBuf {
    if cfg!(windows) {
        app_data(home).join("Claude/claude_desktop_config.json")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Claude/claude_desktop_config.json")
    } else {
        // Claude Desktop is not distributed for Linux; keep the conventional
        // path so status and uninstall can still clean up an earlier install.
        home.join(".config/Claude/claude_desktop_config.json")
    }
}

pub(crate) fn vscode_config(home: &Path) -> PathBuf {
    if cfg!(windows) {
        app_data(home).join("Code/User/mcp.json")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Code/User/mcp.json")
    } else {
        home.join(".config/Code/User/mcp.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_config_path_sits_under_the_given_home() {
        let home = Path::new("/scratch/home");
        for path in [
            claude_config(home),
            codex_config(home),
            gemini_config(home),
            copilot_config(home),
            cursor_config(home),
            antigravity_config(home),
            pi_config(home),
        ] {
            assert!(
                path.starts_with(home),
                "{} escaped the home directory",
                path.display()
            );
        }
    }

    #[test]
    fn desktop_and_vscode_paths_match_the_host_platform() {
        let home = Path::new("/scratch/home");
        let desktop = claude_desktop_config(home);
        let vscode = vscode_config(home);
        assert!(desktop.ends_with("Claude/claude_desktop_config.json"));
        assert!(vscode.ends_with("Code/User/mcp.json"));
        if cfg!(target_os = "macos") {
            assert!(desktop.starts_with(home.join("Library/Application Support")));
            assert!(vscode.starts_with(home.join("Library/Application Support")));
        } else if cfg!(target_os = "linux") {
            assert!(desktop.starts_with(home.join(".config")));
            assert!(vscode.starts_with(home.join(".config")));
        }
    }

    #[test]
    fn antigravity_never_targets_a_back_compat_symlink_directory() {
        let path = antigravity_config(Path::new("/scratch/home"));
        let text = path.display().to_string();
        assert!(text.ends_with(".gemini/config/mcp_config.json"));
        for severed in ["antigravity/", "antigravity-cli/", "antigravity-ide/"] {
            assert!(!text.contains(severed), "would sever {severed}");
        }
    }
}
