//! The supported harnesses, as a table.
//!
//! Every harness owns exactly **one** artifact: the MCP server registration in
//! its user-scope config. Nothing else is written — prose instruction blocks,
//! `@import` lines and skill files are reported by [`super::residue`] and never
//! modified, because they are shared with other tools or owned by a separate
//! distribution channel (ClawHub / Hermes / openclaw own skills).
//!
//! One artifact per harness is why there is no state-folding function here: a
//! harness's state simply *is* its artifact's state.

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
}

pub(crate) const HARNESSES: [HarnessSpec; 8] = [
    HarnessSpec {
        id: "claude",
        name: "Claude Code",
        artifact: Artifact::McpJson,
        config_path: paths::claude_config,
        active_msg: "MCP server registered in ~/.claude.json",
        absent_msg: "no MCP server entry in ~/.claude.json",
        detect: detect_claude,
        note: no_note,
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

fn detect_claude(home: &Path) -> bool {
    home.join(".claude").is_dir()
}

fn detect_codex(home: &Path) -> bool {
    home.join(".codex").is_dir()
}

fn detect_gemini(home: &Path) -> bool {
    home.join(".gemini").is_dir()
}

fn detect_copilot(home: &Path) -> bool {
    home.join(".copilot").is_dir()
}

fn detect_cursor(home: &Path) -> bool {
    home.join(".cursor").is_dir()
}

fn detect_claude_desktop(home: &Path) -> bool {
    parent_is_dir(&paths::claude_desktop_config(home))
}

fn detect_vscode(home: &Path) -> bool {
    parent_is_dir(&paths::vscode_config(home))
}

fn parent_is_dir(path: &Path) -> bool {
    path.parent().is_some_and(Path::is_dir)
}

/// Deliberately not "`~/.gemini` exists": Gemini CLI creates that directory, so
/// keying off it would pre-check Antigravity for every Gemini user.
fn detect_antigravity(home: &Path) -> bool {
    migration_marker(home).exists() || antigravity_data_dirs(home).any(|dir| dir.is_dir())
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
    fn a_bare_gemini_directory_does_not_look_like_antigravity() {
        let home = tmp_dir("gemini-only");
        fs::create_dir_all(home.join(".gemini")).unwrap();

        let antigravity = spec("antigravity").unwrap();
        let gemini = spec("gemini").unwrap();
        assert!((gemini.detect)(&home));
        assert!(
            !(antigravity.detect)(&home),
            "Gemini CLI's own directory pre-checked Antigravity"
        );
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

    #[test]
    fn only_antigravity_carries_a_note() {
        let home = tmp_dir("notes");
        for spec in HARNESSES.iter().filter(|spec| spec.id != "antigravity") {
            assert_eq!((spec.note)(&home), None, "{} added a note", spec.id);
        }
        fs::remove_dir_all(home).ok();
    }
}
