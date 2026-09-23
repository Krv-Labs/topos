//! The supported harnesses, as a table.
//!
//! Every harness owns exactly **one** artifact: the MCP server registration in
//! its user-scope config. Nothing else is written — prose instruction blocks,
//! `@import` lines and skill files are reported by [`super::residue`] and never
//! modified, because they are shared with other tools or owned by a separate
//! distribution channel (ClawHub / Hermes / openclaw own skills).
//!
//! pi is the single exception, marked by `skill_ref`, and only because it is
//! the single harness with no MCP client: its registration configures nothing
//! that runs until the user installs an adapter extension, so it also gets a
//! reference to an already-installed skill directory in its own settings file
//! ([`super::skills_entry`]). That reference is a path in an array, not skill
//! content — the rule above is intact.
//!
//! One artifact per harness is still why there is no state-folding function
//! here: a harness's state simply *is* its MCP artifact's state, and pi's skill
//! reference is reported on its own line rather than folded into that verdict.
//! Folding it in would let a missing skill mask a working MCP entry, or the
//! reverse.

use std::path::{Path, PathBuf};

use super::artifact::Artifact;
use super::paths;

/// One agent harness and the single MCP registration topos owns in it.
pub(crate) struct HarnessSpec {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) artifact: Artifact,
    pub(crate) config_path: fn(&Path) -> PathBuf,
    /// Rendered after `✓` — says *what* is configured rather than a bare
    /// "configured".
    pub(crate) active_msg: &'static str,
    /// Rendered after `○`.
    pub(crate) absent_msg: &'static str,
    /// True when this harness looks present on the machine. Pre-checks the
    /// interactive menu; never gates writing.
    pub(crate) detect: fn(&Path) -> bool,
    /// A caveat shown in install output **and** in `topos status`, even when the
    /// entry is already active.
    pub(crate) note: fn(&Path) -> Option<String>,
    /// Whether this harness also gets a skill-directory reference in its own
    /// settings file — see [`super::skills_entry`]. A plain `bool` rather than
    /// a second artifact slot because there is exactly one implementation and
    /// exactly one harness that needs it: pi, which has no MCP client, so its
    /// MCP entry alone configures nothing that runs today.
    pub(crate) skill_ref: bool,
}

pub(crate) const HARNESSES: [HarnessSpec; 10] = [
    HarnessSpec {
        id: "claude",
        name: "Claude Code",
        artifact: Artifact::McpJson,
        config_path: paths::claude_config,
        active_msg: "MCP server registered in ~/.claude.json",
        absent_msg: "no MCP server entry in ~/.claude.json",
        detect: detect_claude,
        note: no_note,
        skill_ref: false,
    },
    HarnessSpec {
        id: "claude-desktop",
        name: "Claude Desktop",
        artifact: Artifact::McpJson,
        config_path: paths::claude_desktop_config,
        active_msg: "MCP server registered in the Claude Desktop config",
        absent_msg: "no MCP server entry in the Claude Desktop config",
        detect: detect_claude_desktop,
        note: no_note,
        skill_ref: false,
    },
    HarnessSpec {
        id: "codex",
        name: "Codex CLI",
        artifact: Artifact::McpToml,
        config_path: paths::codex_config,
        active_msg: "[mcp_servers.topos] present in ~/.codex/config.toml",
        absent_msg: "no [mcp_servers.topos] in ~/.codex/config.toml",
        detect: detect_codex,
        note: no_note,
        skill_ref: false,
    },
    HarnessSpec {
        id: "gemini",
        name: "Gemini CLI",
        artifact: Artifact::McpJson,
        config_path: paths::gemini_config,
        active_msg: "MCP server registered in ~/.gemini/settings.json",
        absent_msg: "no MCP server entry in ~/.gemini/settings.json",
        detect: detect_gemini,
        note: no_note,
        skill_ref: false,
    },
    HarnessSpec {
        id: "copilot",
        name: "GitHub Copilot CLI",
        artifact: Artifact::McpJson,
        config_path: paths::copilot_config,
        active_msg: "MCP server registered in ~/.copilot/mcp-config.json",
        absent_msg: "no MCP server entry in ~/.copilot/mcp-config.json",
        detect: detect_copilot,
        note: no_note,
        skill_ref: false,
    },
    HarnessSpec {
        id: "cursor",
        name: "Cursor",
        artifact: Artifact::McpJson,
        config_path: paths::cursor_config,
        active_msg: "MCP server registered in ~/.cursor/mcp.json",
        absent_msg: "no MCP server entry in ~/.cursor/mcp.json",
        detect: detect_cursor,
        note: no_note,
        skill_ref: false,
    },
    HarnessSpec {
        id: "vscode",
        name: "VS Code",
        artifact: Artifact::VsCodeJsonc,
        config_path: paths::vscode_config,
        active_msg: "servers.topos present in the VS Code user mcp.json",
        absent_msg: "no servers.topos in the VS Code user mcp.json",
        detect: detect_vscode,
        note: no_note,
        skill_ref: false,
    },
    HarnessSpec {
        id: "antigravity",
        name: "Google Antigravity",
        artifact: Artifact::McpJson,
        config_path: paths::antigravity_config,
        active_msg: "MCP server registered in ~/.gemini/config/mcp_config.json",
        absent_msg: "no MCP server entry in ~/.gemini/config/mcp_config.json",
        detect: detect_antigravity,
        note: antigravity_note,
        skill_ref: false,
    },
    HarnessSpec {
        id: "pi",
        name: "pi",
        artifact: Artifact::McpJson,
        config_path: paths::pi_config,
        active_msg: "MCP server registered in ~/.pi/agent/mcp.json",
        absent_msg: "no MCP server entry in ~/.pi/agent/mcp.json",
        detect: detect_pi,
        note: pi_note,
        skill_ref: true,
    },
    HarnessSpec {
        id: "opencode",
        name: "OpenCode",
        artifact: Artifact::OpenCodeJsonc,
        config_path: paths::opencode_config,
        active_msg: "mcp.topos present in the OpenCode global config",
        absent_msg: "no mcp.topos in the OpenCode global config",
        detect: detect_opencode,
        note: no_note,
        skill_ref: false,
    },
];

/// Every harness id, in table order — the `--all` set and the `--help` list.
pub(crate) fn ids() -> [&'static str; HARNESSES.len()] {
    let mut out = [""; HARNESSES.len()];
    for (slot, spec) in out.iter_mut().zip(HARNESSES.iter()) {
        *slot = spec.id;
    }
    out
}

pub(crate) fn spec(id: &str) -> Option<&'static HarnessSpec> {
    HARNESSES.iter().find(|spec| spec.id == id)
}

fn no_note(_home: &Path) -> Option<String> {
    None
}

/// Helper to check if a file exists and has executable permissions.
fn file_is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        meta.mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        // On Windows, check for known executable extensions since there's no
        // executable bit. PATHEXT is not reliably available here.
        path.extension()
            .and_then(|e| e.to_str())
            .map(|ext| matches!(ext.to_lowercase().as_str(), "exe" | "cmd" | "bat" | "com"))
            .unwrap_or(false)
    }
    #[cfg(not(any(unix, windows)))]
    {
        // Conservative default: assume not executable on unknown platforms.
        false
    }
}

/// True when `name` (or `<name>.exe`, `<name>.cmd`, `<name>.bat` on Windows)
/// is found in an absolute directory on `$PATH` and is executable.
///
/// Relative PATH entries (e.g., `.` or `./bin`) are skipped for security:
/// they would resolve against the current working directory, which an
/// attacker could control.
fn binary_on_path(name: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path_var) {
        if !dir.is_absolute() {
            continue;
        }
        if file_is_executable(&dir.join(name)) {
            return true;
        }
        #[cfg(windows)]
        {
            for ext in ["exe", "cmd", "bat"] {
                if file_is_executable(&dir.join(format!("{name}.{ext}"))) {
                    return true;
                }
            }
        }
    }
    false
}

/// True when `name` is found in standard user bin directories under `home`
/// (e.g. `~/.local/bin`, `~/.cargo/bin`, `~/.opencode/bin`).
fn binary_in_home(name: &str, home: &Path) -> bool {
    home_bin_candidates(name, home)
        .iter()
        .any(|p| file_is_executable(p))
        || windows_user_bin_installed(name)
}

fn home_bin_candidates(name: &str, home: &Path) -> Vec<PathBuf> {
    let bases = [
        home.join(".local/bin").join(name),
        home.join(".cargo/bin").join(name),
        home.join(".opencode/bin").join(name),
        home.join("bin").join(name),
    ];
    #[cfg(not(windows))]
    {
        bases.to_vec()
    }
    #[cfg(windows)]
    {
        let mut all = Vec::new();
        for base in &bases {
            all.push(base.clone());
            for ext in ["exe", "cmd", "bat"] {
                all.push(base.with_extension(ext));
            }
        }
        all
    }
}

#[cfg(not(windows))]
fn windows_user_bin_installed(_name: &str) -> bool {
    false
}

#[cfg(windows)]
fn windows_user_bin_installed(name: &str) -> bool {
    if let Some(app_data) = std::env::var_os("APPDATA") {
        let npm_bin = Path::new(&app_data).join("npm");
        for ext in ["cmd", "exe", "bat"] {
            if file_is_executable(&npm_bin.join(format!("{name}.{ext}"))) {
                return true;
            }
        }
    }
    if let Some(local_app_data) = std::env::var_os("LOCALAPPDATA") {
        // npm can also install to %LOCALAPPDATA%\npm
        let npm_bin = Path::new(&local_app_data).join("npm");
        for ext in ["cmd", "exe", "bat"] {
            if file_is_executable(&npm_bin.join(format!("{name}.{ext}"))) {
                return true;
            }
        }
        let win_apps = Path::new(&local_app_data).join("Microsoft/WindowsApps");
        for ext in ["exe", "cmd"] {
            if file_is_executable(&win_apps.join(format!("{name}.{ext}"))) {
                return true;
            }
        }
    }
    false
}

fn is_real_home(home: &Path) -> bool {
    paths::home_dir().is_ok_and(|real| real == home)
}

/// True when `name` is executable on `$PATH` or in standard user bin directories.
fn binary_installed(name: &str, home: &Path) -> bool {
    binary_in_home(name, home) || (is_real_home(home) && binary_on_path(name))
}

/// True when a GUI desktop application is installed on the host.
fn app_installed(app_name: &str, home: &Path) -> bool {
    if !is_real_home(home) {
        return home
            .join("Applications")
            .join(format!("{app_name}.app"))
            .is_dir();
    }
    platform_app_installed(app_name)
}

#[cfg(target_os = "macos")]
fn platform_app_installed(app_name: &str) -> bool {
    let bundle = format!("{app_name}.app");
    Path::new("/Applications").join(&bundle).is_dir()
        || Path::new("/System/Applications").join(&bundle).is_dir()
}

#[cfg(windows)]
fn platform_app_installed(app_name: &str) -> bool {
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let p = Path::new(&local).join("Programs").join(app_name);
        if p.exists() {
            return true;
        }
    }
    for key in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(pf) = std::env::var_os(key) {
            let p = Path::new(&pf).join(app_name);
            if p.exists() {
                return true;
            }
        }
    }
    false
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_app_installed(app_name: &str) -> bool {
    let lower = app_name.to_lowercase();
    Path::new("/usr/share").join(&lower).is_dir()
        || Path::new("/opt").join(app_name).is_dir()
        || Path::new("/opt").join(&lower).is_dir()
}

#[cfg(not(any(target_os = "macos", windows, all(unix, not(target_os = "macos")))))]
fn platform_app_installed(_app_name: &str) -> bool {
    false
}

fn detect_claude(home: &Path) -> bool {
    binary_installed("claude", home)
}

fn detect_codex(home: &Path) -> bool {
    binary_installed("codex", home)
}

fn detect_gemini(home: &Path) -> bool {
    binary_installed("gemini", home)
}

fn detect_copilot(home: &Path) -> bool {
    binary_installed("copilot", home)
}

fn detect_cursor(home: &Path) -> bool {
    binary_installed("cursor", home) || app_installed("Cursor", home)
}

fn detect_pi(home: &Path) -> bool {
    binary_installed("pi", home)
}

fn detect_claude_desktop(home: &Path) -> bool {
    app_installed("Claude", home)
}

fn detect_vscode(home: &Path) -> bool {
    binary_installed("code", home) || app_installed("Visual Studio Code", home)
}

fn detect_opencode(home: &Path) -> bool {
    binary_installed("opencode", home)
}

/// Deliberately not "`~/.gemini` exists": Gemini CLI creates that directory, so
/// keying off it would pre-check Antigravity for every Gemini user.
fn detect_antigravity(home: &Path) -> bool {
    binary_installed("agy", home)
        || app_installed("Antigravity", home)
        || migration_marker(home).exists()
        || antigravity_data_dirs(home).any(|dir| dir.is_dir())
}

/// Antigravity's own migration writes this marker once it has moved MCP config
/// into `~/.gemini/config/`.
fn migration_marker(home: &Path) -> PathBuf {
    home.join(".gemini/config/.migrated")
}

fn antigravity_data_dirs(home: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    ["antigravity", "antigravity-cli", "antigravity-ide"]
        .into_iter()
        .map(move |name| home.join(".gemini").join(name))
}

/// Before Antigravity has migrated, its next launch whole-file-replaces
/// `~/.gemini/config/mcp_config.json` from its app data directory with no merge
/// — tested, and a pre-written topos entry was destroyed. Install still writes,
/// but a `✓` without this warning would be a silent failure.
fn antigravity_note(home: &Path) -> Option<String> {
    if migration_marker(home).exists() {
        return None;
    }
    let unmigrated = antigravity_data_dirs(home)
        .map(|dir| dir.join("mcp_config.json"))
        .any(|path| is_regular_file(&path));
    unmigrated.then(|| {
        "Antigravity has not migrated its config yet — launch Antigravity once, then re-run \
         `topos install antigravity`, or this entry will be discarded."
            .to_string()
    })
}

/// pi is the one harness with no MCP client of its own — its README says
/// "No MCP. […] build an extension that adds MCP support." The registration
/// is read by the `pi-mcp-adapter` extension, so the entry alone does nothing
/// until that extension is installed. Unconditional, because nothing on disk
/// distinguishes "adapter installed" from "not".
fn pi_note(_home: &Path) -> Option<String> {
    Some(
        "pi reads MCP servers only through its adapter extension — run \
         `pi install npm:pi-mcp-adapter` if you have not already, or this entry is inert."
            .to_string(),
    )
}

/// A real file rather than one of the back-compat symlinks Antigravity's
/// migration leaves behind pointing into `~/.gemini/config/`.
fn is_regular_file(path: &Path) -> bool {
    std::fs::read_link(path).is_err() && path.is_file()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::commands::install::testing::tmp_dir;

    #[test]
    fn ids_are_unique_and_match_the_table_order() {
        let ids = ids();
        assert_eq!(ids.len(), HARNESSES.len());
        for (index, entry) in HARNESSES.iter().enumerate() {
            assert_eq!(ids[index], entry.id);
            assert_eq!(spec(entry.id).map(|found| found.name), Some(entry.name));
        }
        let mut sorted = ids.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate harness id");
    }

    #[test]
    fn every_harness_has_a_distinct_config_path_and_a_specific_message() {
        let home = Path::new("/scratch/home");
        let mut seen = Vec::new();
        for spec in &HARNESSES {
            let path = (spec.config_path)(home);
            assert!(!seen.contains(&path), "{} shares a config path", spec.id);
            seen.push(path);
            assert_ne!(spec.active_msg, "configured", "{} is not specific", spec.id);
            assert!(!spec.absent_msg.is_empty());
        }
    }

    #[test]
    fn a_bare_pi_directory_does_not_detect_pi_until_binary_installed() {
        let home = tmp_dir("pi-detect");
        fs::create_dir_all(home.join(".pi/agent/skills")).unwrap();
        let pi = spec("pi").unwrap();
        // A bare directory or skill symlink farm must not detect pi
        assert!(!(pi.detect)(&home));

        // Adding an executable binary in user bins detects it
        let bin_dir = home.join(".local/bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let bin_path = bin_dir.join("pi");
        fs::write(&bin_path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!((pi.detect)(&home));
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_bare_gemini_directory_does_not_look_like_antigravity() {
        let home = tmp_dir("gemini-only");
        fs::create_dir_all(home.join(".gemini")).unwrap();

        let antigravity = spec("antigravity").unwrap();
        let gemini = spec("gemini").unwrap();
        // Neither is detected from a bare directory
        assert!(!(gemini.detect)(&home));
        assert!(
            !(antigravity.detect)(&home),
            "Gemini CLI's own directory pre-checked Antigravity"
        );
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn opencode_detected_when_binary_present() {
        let home = tmp_dir("opencode-detect");
        let opencode = spec("opencode").unwrap();
        assert!(!(opencode.detect)(&home));

        let bin_dir = home.join(".opencode/bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let bin_path = bin_dir.join("opencode");
        fs::write(&bin_path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&bin_path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert!((opencode.detect)(&home));
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn the_migration_marker_makes_antigravity_detected_and_silences_the_note() {
        let home = tmp_dir("migrated");
        fs::create_dir_all(home.join(".gemini/config")).unwrap();
        fs::write(home.join(".gemini/config/.migrated"), "").unwrap();

        let antigravity = spec("antigravity").unwrap();
        assert!((antigravity.detect)(&home));
        assert_eq!((antigravity.note)(&home), None);
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn an_unmigrated_install_is_detected_and_warns_that_the_entry_is_at_risk() {
        let home = tmp_dir("unmigrated");
        fs::create_dir_all(home.join(".gemini/antigravity")).unwrap();
        fs::write(home.join(".gemini/antigravity/mcp_config.json"), "{}").unwrap();

        let antigravity = spec("antigravity").unwrap();
        assert!((antigravity.detect)(&home));
        let note = (antigravity.note)(&home).expect("unmigrated install must warn");
        assert!(note.contains("launch Antigravity once"), "{note}");
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_back_compat_symlink_is_not_mistaken_for_an_unmigrated_config() {
        let home = tmp_dir("symlinked");
        fs::create_dir_all(home.join(".gemini/config")).unwrap();
        fs::create_dir_all(home.join(".gemini/antigravity")).unwrap();
        fs::write(home.join(".gemini/config/mcp_config.json"), "{}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            home.join(".gemini/config/mcp_config.json"),
            home.join(".gemini/antigravity/mcp_config.json"),
        )
        .unwrap();

        // No `.migrated` marker, but the only candidate is the migration's own
        // back-compat symlink, so there is nothing left to overwrite us.
        #[cfg(unix)]
        assert_eq!((spec("antigravity").unwrap().note)(&home), None);
        fs::remove_dir_all(home).ok();
    }

    /// Antigravity's note is conditional on an unmigrated config; pi's is
    /// unconditional, because its adapter extension leaves no marker on disk.
    /// Every other harness stays quiet.
    #[test]
    fn only_antigravity_and_pi_carry_a_note() {
        let home = tmp_dir("notes");
        for spec in HARNESSES
            .iter()
            .filter(|spec| !matches!(spec.id, "antigravity" | "pi"))
        {
            assert_eq!((spec.note)(&home), None, "{} added a note", spec.id);
        }
        assert!(
            (spec("pi").unwrap().note)(&home).is_some_and(|note| note.contains("pi-mcp-adapter"))
        );
        fs::remove_dir_all(home).ok();
    }
}
