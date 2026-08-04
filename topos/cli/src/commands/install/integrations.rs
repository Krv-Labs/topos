//! The harness table: which agent harnesses Topos supports, and what it owns
//! inside each one.
//!
//! [`HARNESSES`] is the single source of truth. Each entry names its display
//! label, the directory whose presence means "this harness is installed on
//! this machine", and the [`Artifact`]s Topos owns inside it. Install,
//! uninstall, status, detection, and backup purging are all loops over
//! `harness.artifacts`, so adding a harness is one table entry rather than an
//! edit in each of those five places.
//!
//! Each artifact kind delegates to the matching primitive in
//! [`edits`](super::edits); file ownership lives in
//! [`ownership`](super::ownership).
//!
//! The mechanism is derived from the harness-installer pattern in
//! sgathrid/brian (`wikicli/lifecycle/`), but brian injects session-start
//! context via hooks; Topos exposes MCP tools, so these adapters register an
//! MCP server (or, for harnesses without MCP support, drop a marked
//! instructions block or a skill file) rather than porting the hook
//! machinery.
//!
//! Schema notes:
//! - Claude Code stores user-scope MCP servers in `~/.claude.json`, not
//!   `~/.claude/settings.json` (settings.json is hooks/permissions only).
//! - Codex CLI, Gemini CLI, and Cursor accept the same
//!   `{"command": …, "args": ["mcp"]}` shape Claude Desktop uses.

use std::path::{Path, PathBuf};

use super::edits::{
    import_state, json_mcp_state, marker_state, remove_import_line, remove_marker_block,
    remove_mcp_entry, remove_toml_mcp_entry, remove_written_file, set_import_line,
    set_marker_block, set_mcp_entry, set_toml_mcp_entry, skill_state, toml_mcp_state, write_skill,
    SKILL_MD,
};
use super::ownership::{
    delete_if_empty_and_owned, delete_text_if_blank_and_owned, record_created_file,
};

// Claude Desktop is not currently distributed for Linux; the non-macOS path
// is kept so status/uninstall can still clean up a config left by an earlier
// install.
const CLAUDE_DESKTOP_CONFIG: &str = if cfg!(target_os = "macos") {
    "Library/Application Support/Claude/claude_desktop_config.json"
} else {
    ".config/Claude/claude_desktop_config.json"
};

const CLAUDE_DESKTOP_DIR: &str = if cfg!(target_os = "macos") {
    "Library/Application Support/Claude"
} else {
    ".config/Claude"
};

/// One thing Topos owns inside a harness's configuration. Paths are relative
/// to the user's home directory.
pub(crate) enum Artifact {
    /// `mcpServers.topos` inside a JSON config.
    JsonMcp(&'static str),
    /// `[mcp_servers.topos]` inside a TOML config.
    TomlMcp(&'static str),
    /// A `<!-- topos:start -->` … `<!-- topos:end -->` block in a Markdown file.
    MarkerBlock(&'static str),
    /// A written-out copy of `SKILL.md`.
    SkillFile(&'static str),
    /// An `@import` line in `host` pointing at a written skill copy at `pointer`.
    ImportLine {
        host: &'static str,
        pointer: &'static str,
    },
}

pub(crate) struct Harness {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    /// Header shown by install/uninstall, e.g. `Claude Code (~/.claude)`.
    pub(crate) label: &'static str,
    /// Directory whose presence means this harness is installed locally.
    pub(crate) detect: &'static str,
    pub(crate) artifacts: &'static [Artifact],
}

pub(crate) const HARNESSES: &[Harness] = &[
    Harness {
        id: "claude",
        name: "Claude Code",
        label: "Claude Code (~/.claude)",
        detect: ".claude",
        artifacts: &[
            Artifact::JsonMcp(".claude.json"),
            Artifact::SkillFile(".claude/skills/topos/SKILL.md"),
        ],
    },
    Harness {
        id: "claude-desktop",
        name: "Claude Desktop App",
        label: "Claude Desktop App",
        detect: CLAUDE_DESKTOP_DIR,
        artifacts: &[Artifact::JsonMcp(CLAUDE_DESKTOP_CONFIG)],
    },
    Harness {
        id: "codex",
        name: "Codex CLI",
        label: "Codex CLI (~/.codex)",
        detect: ".codex",
        artifacts: &[Artifact::TomlMcp(".codex/config.toml")],
    },
    Harness {
        id: "gemini",
        name: "Gemini CLI",
        label: "Gemini CLI (~/.gemini)",
        detect: ".gemini",
        artifacts: &[Artifact::JsonMcp(".gemini/settings.json")],
    },
    Harness {
        id: "copilot",
        name: "GitHub Copilot CLI",
        label: "GitHub Copilot CLI (~/.copilot)",
        detect: ".copilot",
        artifacts: &[Artifact::MarkerBlock(".copilot/copilot-instructions.md")],
    },
    Harness {
        id: "skills",
        name: "Cursor & VS Code",
        label: "Cursor & VS Code (~/.agents/skills)",
        detect: ".agents/skills",
        artifacts: &[
            Artifact::SkillFile(".agents/skills/topos/SKILL.md"),
            Artifact::JsonMcp(".cursor/mcp.json"),
        ],
    },
    Harness {
        id: "antigravity",
        name: "Google Antigravity",
        label: "Google Antigravity (~/.gemini/GEMINI.md)",
        detect: ".gemini",
        artifacts: &[Artifact::ImportLine {
            host: ".gemini/GEMINI.md",
            pointer: ".gemini/topos-skill.md",
        }],
    },
];

pub(crate) fn harness(id: &str) -> Option<&'static Harness> {
    HARNESSES.iter().find(|h| h.id == id)
}

pub(crate) fn supported_ids() -> Vec<&'static str> {
    HARNESSES.iter().map(|h| h.id).collect()
}

impl Harness {
    /// Where install/uninstall look to decide whether this harness exists on
    /// this machine at all.
    pub(crate) fn detect_dir(&self, home: &Path) -> PathBuf {
        home.join(self.detect)
    }

    pub(crate) fn is_detected(&self, home: &Path) -> bool {
        self.detect_dir(home).exists()
    }

    /// Overall state, folding together every artifact this harness owns.
    pub(crate) fn state(&self, home: &Path) -> State {
        self.artifacts
            .iter()
            .map(|artifact| artifact.state(home))
            .reduce(combine)
            .unwrap_or(State::Absent)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum State {
    Active,
    Stale,
    Absent,
}

/// What a single install or uninstall step actually did — reported verbatim,
/// so the output never claims a write or a removal that did not happen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Outcome {
    /// The file was written.
    Changed,
    /// Already in the requested state; nothing to do.
    Unchanged,
    /// Locally modified, so it was deliberately left alone.
    Preserved,
}

fn changed_or_not(changed: bool) -> Outcome {
    if changed {
        Outcome::Changed
    } else {
        Outcome::Unchanged
    }
}

/// A harness is only [`State::Active`] once every artifact it owns is, and
/// only [`State::Absent`] while none of them are; anything in between needs
/// repair.
pub(crate) fn combine(a: State, b: State) -> State {
    match (a, b) {
        (State::Active, State::Active) => State::Active,
        (State::Absent, State::Absent) => State::Absent,
        _ => State::Stale,
    }
}

pub(crate) fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| "cannot resolve home directory (HOME is unset)".to_string())
}

impl Artifact {
    /// The primary file this artifact lives in.
    pub(crate) fn path(&self, home: &Path) -> PathBuf {
        let relative = match self {
            Artifact::JsonMcp(path)
            | Artifact::TomlMcp(path)
            | Artifact::MarkerBlock(path)
            | Artifact::SkillFile(path) => path,
            Artifact::ImportLine { host, .. } => host,
        };
        home.join(relative)
    }

    /// The written skill copy an [`Artifact::ImportLine`] points at.
    fn pointer(&self, home: &Path) -> PathBuf {
        match self {
            Artifact::ImportLine { pointer, .. } => home.join(pointer),
            _ => self.path(home),
        }
    }

    /// Every file this artifact touches — used to purge backups.
    pub(crate) fn paths(&self, home: &Path) -> Vec<PathBuf> {
        match self {
            Artifact::ImportLine { .. } => vec![self.path(home), self.pointer(home)],
            _ => vec![self.path(home)],
        }
    }

    /// Human-readable phrase used in every install/uninstall line.
    pub(crate) fn describe(&self, home: &Path) -> String {
        let path = self.path(home);
        match self {
            Artifact::JsonMcp(_) => format!("MCP server entry in {}", path.display()),
            Artifact::TomlMcp(_) => format!("[mcp_servers.topos] in {}", path.display()),
            Artifact::MarkerBlock(_) => format!("instruction block in {}", path.display()),
            Artifact::SkillFile(_) => format!("skill file {}", path.display()),
            Artifact::ImportLine { .. } => format!("@import line in {}", path.display()),
        }
    }

    pub(crate) fn state(&self, home: &Path) -> State {
        let path = self.path(home);
        match self {
            Artifact::JsonMcp(_) => json_mcp_state(&path),
            Artifact::TomlMcp(_) => toml_mcp_state(&path),
            Artifact::MarkerBlock(_) => marker_state(&path),
            Artifact::SkillFile(_) => skill_state(&path),
            Artifact::ImportLine { .. } => import_state(&path, &self.pointer(home)),
        }
    }

    pub(crate) fn install(&self, home: &Path, harness: &str) -> Result<Outcome, String> {
        let path = self.path(home);
        let existed = path.is_file();
        let changed = match self {
            Artifact::JsonMcp(_) => set_mcp_entry(&path)?,
            Artifact::TomlMcp(_) => set_toml_mcp_entry(&path)?,
            Artifact::MarkerBlock(_) => set_marker_block(&path)?,
            Artifact::SkillFile(_) => write_skill(&path)?,
            Artifact::ImportLine { .. } => set_import_line(&path, &self.pointer(home))?,
        };
        if changed && !existed {
            record_created_file(home, harness, &path)?;
        }
        Ok(changed_or_not(changed))
    }

    pub(crate) fn remove(
        &self,
        home: &Path,
        harness: &str,
        dry_run: bool,
    ) -> Result<Outcome, String> {
        let path = self.path(home);
        match self {
            Artifact::JsonMcp(_) => {
                let removed = remove_mcp_entry(&path, dry_run)?;
                delete_if_empty_and_owned(&path, home, harness, dry_run)?;
                Ok(changed_or_not(removed))
            }
            Artifact::TomlMcp(_) => {
                let removed = remove_toml_mcp_entry(&path, dry_run)?;
                delete_text_if_blank_and_owned(&path, home, harness, dry_run)?;
                Ok(changed_or_not(removed))
            }
            Artifact::MarkerBlock(_) => {
                let removed = remove_marker_block(&path, dry_run)?;
                delete_text_if_blank_and_owned(&path, home, harness, dry_run)?;
                Ok(changed_or_not(removed))
            }
            Artifact::SkillFile(_) => remove_written_file(&path, SKILL_MD, dry_run),
            Artifact::ImportLine { .. } => {
                remove_import_line(&path, &self.pointer(home), home, harness, dry_run)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every harness in the table must be reachable by id and own distinct
    /// files — the check the seven hand-maintained id lists this table
    /// replaced could not make.
    #[test]
    fn every_harness_is_addressable_and_owns_distinct_paths() {
        let home = Path::new("/home/example");
        for id in supported_ids() {
            let entry = harness(id).expect("id resolves to a table entry");
            assert!(!entry.artifacts.is_empty(), "{id} owns nothing");
            let mut paths: Vec<PathBuf> = entry
                .artifacts
                .iter()
                .flat_map(|artifact| artifact.paths(home))
                .collect();
            let total = paths.len();
            paths.sort();
            paths.dedup();
            assert_eq!(paths.len(), total, "{id} lists a path twice");
        }
    }

    /// A partially-configured harness reads as stale, which is what drives
    /// status's repair hint and the menu's pre-selection.
    #[test]
    fn a_harness_is_active_only_once_every_artifact_is() {
        assert_eq!(combine(State::Active, State::Active), State::Active);
        assert_eq!(combine(State::Absent, State::Absent), State::Absent);
        assert_eq!(combine(State::Active, State::Absent), State::Stale);
        assert_eq!(combine(State::Absent, State::Stale), State::Stale);
    }
}
