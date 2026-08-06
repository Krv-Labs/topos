//! The single MCP registration `topos install` owns in a harness config, and
//! the three file shapes real clients use for it.
//!
//! Everything harness-specific beyond the file *format* lives in the
//! [`super::harness`] table; this module knows only how to read, classify,
//! write and remove one server entry. Format-specific work is delegated to
//! [`super::json_entry`] and [`super::toml_entry`].
//!
//! Three rules here are load-bearing, each fixing a defect found by testing
//! the draft against real clients:
//!
//! - **The entry is `{command, args}` with no `type`.** A bare pair loads and
//!   connects on Copilot CLI 1.0.73, Gemini CLI 0.53.1 and Claude Code 2.1.221;
//!   Gemini's shipped schema has no `required` array and Claude Desktop's has
//!   no `type` property at all. No literal value is portable either — Copilot's
//!   own writer emits `"type":"local"` where Claude Code's emits `"stdio"`.
//!   VS Code is the sole exception and does take `"type": "stdio"`.
//! - **`args` must be exactly `["mcp"]`.** A bare `topos` prints usage and
//!   exits, which surfaces to the client as `-32000: Connection closed`.
//! - **Comparison is field-wise on `command` and `args`, never whole-value
//!   equality.** Clients normalize entries and add their own keys; an equality
//!   check pins those harnesses at `Incomplete` forever and rewrites the file
//!   on every run.

use std::path::Path;

use super::binary::same_file;
use super::fsops::WriteOutcome;
use super::{json_entry, toml_entry};

/// Both the MCP server name and the ownership marker: `topos install` only ever
/// reads, writes or removes the entry under this key.
pub(crate) const SERVER_KEY: &str = "topos";

/// The only argument list a topos MCP server is ever registered with.
pub(crate) const MCP_ARGS: [&str; 1] = ["mcp"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum State {
    /// Entry present, ours, and its `command` resolves to this binary.
    Active,
    /// Ours but in need of repair — `topos install` fixes it. Never "run
    /// uninstall": there is nothing here a user has to clean up by hand.
    Incomplete,
    /// The file will not parse, the `topos` key holds something topos did not
    /// write, or a VS Code `mcp.json` carries comments. topos reports the path
    /// and writes nothing.
    Conflict,
    /// No entry.
    Absent,
}

/// The result of inspecting one harness's registration.
pub(crate) struct Inspection {
    pub(crate) state: State,
    /// Why, for `Incomplete` and `Conflict`. Always `None` for the other two —
    /// their message comes from the harness spec instead.
    pub(crate) detail: Option<String>,
}

impl Inspection {
    pub(crate) fn plain(state: State) -> Self {
        Self {
            state,
            detail: None,
        }
    }

    pub(crate) fn incomplete(detail: String) -> Self {
        Self {
            state: State::Incomplete,
            detail: Some(detail),
        }
    }

    pub(crate) fn conflict(detail: String) -> Self {
        Self {
            state: State::Conflict,
            detail: Some(detail),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Artifact {
    /// `mcpServers.topos` in plain JSON — Claude Code, Claude Desktop, Gemini
    /// CLI, Copilot CLI, Cursor, Antigravity.
    McpJson,
    /// `[mcp_servers.topos]` in TOML, edited through `toml_edit` so comments
    /// and formatting survive.
    McpToml,
    /// `servers.topos` carrying `"type": "stdio"`, in a file that may contain
    /// comments and trailing commas.
    VsCodeJsonc,
}

impl Artifact {
    /// The table holding server entries: `servers` for VS Code, `mcpServers`
    /// for JSON clients, `mcp_servers` for Codex's TOML.
    pub(crate) fn container_key(self) -> &'static str {
        match self {
            Artifact::McpJson => "mcpServers",
            Artifact::McpToml => "mcp_servers",
            Artifact::VsCodeJsonc => "servers",
        }
    }

    /// Whether this client wants an explicit transport. VS Code alone does.
    pub(crate) fn wants_stdio_type(self) -> bool {
        matches!(self, Artifact::VsCodeJsonc)
    }

    /// Whether the file may carry comments and trailing commas.
    pub(crate) fn is_jsonc(self) -> bool {
        matches!(self, Artifact::VsCodeJsonc)
    }

    pub(crate) fn inspect(self, path: &Path, binary: &Path) -> Inspection {
        match self {
            Artifact::McpToml => toml_entry::inspect(path, binary),
            _ => json_entry::inspect(self, path, binary),
        }
    }

    /// Register the server, backing up first only when there was no entry of
    /// ours to begin with. `Ok(None)` means the entry was already correct and
    /// nothing was written.
    pub(crate) fn apply(self, path: &Path, binary: &Path) -> Result<Option<WriteOutcome>, String> {
        let inspection = self.inspect(path, binary);
        match inspection.state {
            State::Active => Ok(None),
            State::Conflict => Err(inspection
                .detail
                .unwrap_or_else(|| format!("{} cannot be updated safely", path.display()))),
            // Back up only content topos did not write. Anything other than
            // `Absent` means the file already holds our entry, so a snapshot
            // taken now would capture our own output and destroy the pristine
            // pre-install one — which self-healing makes a routine event.
            state => self.write(path, binary, state == State::Absent).map(Some),
        }
    }

    fn write(self, path: &Path, binary: &Path, backup: bool) -> Result<WriteOutcome, String> {
        match self {
            Artifact::McpToml => toml_entry::write(path, binary, backup),
            _ => json_entry::write(self, path, binary, backup),
        }
    }

    /// Remove a topos-owned registration. `Ok(false)` when there was nothing of
    /// ours to remove — a hand-made entry under the `topos` key is left alone.
    pub(crate) fn remove(self, path: &Path, dry_run: bool) -> Result<bool, String> {
        match self {
            Artifact::McpToml => toml_entry::remove(path, dry_run),
            _ => json_entry::remove(self, path, dry_run),
        }
    }

    /// MCP keys other than `topos` that also point at the topos binary.
    /// Reported as duplicate registrations — two of them mean duplicate tool
    /// names and two `topos mcp` processes — but never renamed or removed:
    /// they are the user's entries.
    pub(crate) fn duplicate_keys(self, path: &Path, binary: &Path) -> Vec<String> {
        match self {
            Artifact::McpToml => toml_entry::duplicate_keys(path, binary),
            _ => json_entry::duplicate_keys(self, path, binary),
        }
    }
}

/// True when `command` names the topos executable, by file name alone.
///
/// Deliberately does not require the path to still resolve: a drifted entry is
/// still ours to repair or remove, including the bare `"topos"` an earlier draft
/// of this command wrote.
pub(crate) fn names_topos(command: &str) -> bool {
    matches!(
        Path::new(command).file_name().and_then(|n| n.to_str()),
        Some("topos" | "topos.exe")
    )
}

/// True when a foreign MCP key's command is the topos binary — either by name
/// or because it resolves to the same file.
pub(crate) fn points_at_topos(command: &str, binary: &Path) -> bool {
    let named = matches!(
        Path::new(command).file_name().and_then(|n| n.to_str()),
        Some("topos" | "topos.exe" | "topos-mcp" | "topos-mcp.exe")
    );
    named || same_file(Path::new(command), binary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_vscode_declares_a_transport_and_uses_the_servers_key() {
        assert_eq!(Artifact::McpJson.container_key(), "mcpServers");
        assert_eq!(Artifact::McpToml.container_key(), "mcp_servers");
        assert_eq!(Artifact::VsCodeJsonc.container_key(), "servers");
        assert!(Artifact::VsCodeJsonc.wants_stdio_type());
        assert!(!Artifact::McpJson.wants_stdio_type());
        assert!(!Artifact::McpToml.wants_stdio_type());
        assert!(Artifact::VsCodeJsonc.is_jsonc());
        assert!(!Artifact::McpJson.is_jsonc());
    }

    #[test]
    fn ownership_is_by_file_name_so_a_drifted_entry_is_still_ours() {
        assert!(names_topos("topos"));
        assert!(names_topos("/opt/homebrew/bin/topos"));
        assert!(names_topos("/nonexistent/path/topos"));
        assert!(!names_topos("/usr/local/bin/other"));
        assert!(!names_topos(""));
    }

    #[test]
    fn duplicate_detection_also_catches_the_hand_made_topos_mcp_command() {
        let binary = Path::new("/opt/homebrew/bin/topos");
        assert!(points_at_topos("topos-mcp", binary));
        assert!(points_at_topos("/usr/local/bin/topos", binary));
        assert!(!points_at_topos("uvx", binary));
    }
}
