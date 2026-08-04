//! The harness table and the persistence primitives `install` / `uninstall`
//! / `status` build on: atomic writes with backups, marker-based idempotent
//! edits, and a small ownership-tracking file so uninstall only ever deletes
//! what a previous `topos install` actually created.
//!
//! [`HARNESSES`] is the single source of truth. Each entry names its display
//! label, the directory whose presence means "this harness is installed on
//! this machine", and the [`Artifact`]s Topos owns inside it. Install,
//! uninstall, status, detection, and backup purging are all loops over
//! `harness.artifacts`, so adding a harness is one table entry rather than an
//! edit in each of those five places.
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

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use toml_edit::{value, Array, DocumentMut, Item, Table};

/// Embedded so a globally-installed `topos` binary (cargo install, the curl
/// installer, a package manager) can drop the skill file without a checkout
/// of this repository on disk.
pub(crate) const SKILL_MD: &str = include_str!("../../../../../skills/topos/SKILL.md");

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
    /// A written-out copy of [`SKILL_MD`].
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

fn combine(a: State, b: State) -> State {
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
    fn pointer(&self, home: &Path) -> Option<PathBuf> {
        match self {
            Artifact::ImportLine { pointer, .. } => Some(home.join(pointer)),
            _ => None,
        }
    }

    /// Every file this artifact touches — used to purge backups.
    pub(crate) fn paths(&self, home: &Path) -> Vec<PathBuf> {
        let mut paths = vec![self.path(home)];
        paths.extend(self.pointer(home));
        paths
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
            Artifact::ImportLine { .. } => {
                import_state(&path, &self.pointer(home).unwrap_or_else(|| path.clone()))
            }
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
            Artifact::ImportLine { .. } => {
                set_import_line(&path, &self.pointer(home).unwrap_or_else(|| path.clone()))?
            }
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
            Artifact::ImportLine { .. } => remove_import_line(
                &path,
                &self.pointer(home).unwrap_or_else(|| path.clone()),
                home,
                harness,
                dry_run,
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Atomic writes and backups
// ---------------------------------------------------------------------------

pub(crate) fn backup_path(path: &Path) -> PathBuf {
    suffixed(path, ".topos.backup")
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    path.with_file_name(name)
}

/// Write via a temp file + rename, optionally snapshotting the previous
/// contents first. Creates parent directories as needed.
///
/// Two properties matter beyond atomicity. The target's permissions are
/// carried onto the replacement, because several of these configs are
/// deliberately `0600` (`~/.claude.json` holds account state) and a fresh
/// temp file would otherwise widen them to the process umask. And an existing
/// backup is never overwritten, so the snapshot stays the pre-Topos original
/// instead of being replaced by already-modified content on a later write.
fn atomic_write(path: &Path, contents: &str, backup: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    let previous = fs::metadata(path).ok().filter(|meta| meta.is_file());
    if backup && previous.is_some() {
        let snapshot = backup_path(path);
        if !snapshot.exists() {
            fs::copy(path, &snapshot).map_err(|e| format!("backing up {}: {e}", path.display()))?;
        }
    }
    let tmp = suffixed(path, ".topos.tmp");
    fs::write(&tmp, contents).map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    if let Some(metadata) = previous {
        fs::set_permissions(&tmp, metadata.permissions())
            .map_err(|e| format!("setting permissions on {}: {e}", tmp.display()))?;
    }
    fs::rename(&tmp, path).map_err(|e| format!("replacing {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// The MCP server entry
// ---------------------------------------------------------------------------

/// Absolute path to the running binary, so harnesses launched by the desktop
/// environment rather than a login shell (Claude Desktop, Cursor,
/// Antigravity) can spawn it without a usable `PATH`. Falls back to the bare
/// name if the path cannot be resolved.
fn topos_command() -> String {
    std::env::current_exe()
        .and_then(|path| path.canonicalize())
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "topos".to_string())
}

fn mcp_entry() -> Value {
    json!({ "command": topos_command(), "args": ["mcp"] })
}

/// Whether a command string launches the Topos binary. Any `topos` counts,
/// wherever it lives, so re-installing from a different prefix (or over an
/// entry written by `claude mcp add`) is neither reported as stale nor
/// rewritten. The running binary's own path counts regardless of its file
/// name, which keeps write-then-detect consistent when that name is not
/// literally `topos` (a test binary, a renamed copy).
fn is_topos_binary(command: &str) -> bool {
    Path::new(command).file_stem() == Some(std::ffi::OsStr::new("topos"))
        || command == topos_command()
}

fn is_topos_mcp_entry(entry: &Value) -> bool {
    let command_ok = entry
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(is_topos_binary);
    let args_ok = entry
        .get("args")
        .and_then(Value::as_array)
        .is_some_and(|args| args.iter().filter_map(Value::as_str).eq(["mcp"]));
    command_ok && args_ok
}

/// Read a JSON object config, treating a missing file as empty. Errors on
/// unreadable/invalid content or a non-object top level so callers never
/// silently clobber something they can't safely parse.
fn read_json_object(path: &Path) -> Result<Map<String, Value>, String> {
    if !path.is_file() {
        return Ok(Map::new());
    }
    let text = fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str(&text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!(
            "{} top-level value must be an object",
            path.display()
        )),
        Err(e) => Err(format!("parsing {}: {e}", path.display())),
    }
}

fn write_json_object(path: &Path, data: &Map<String, Value>, backup: bool) -> Result<(), String> {
    let contents = serde_json::to_string_pretty(data).map_err(|e| e.to_string())? + "\n";
    atomic_write(path, &contents, backup)
}

/// `Active` when `mcpServers.topos` already launches Topos, `Stale` when the
/// key exists pointing at something else (or the file fails to parse),
/// `Absent` otherwise.
pub(crate) fn json_mcp_state(path: &Path) -> State {
    match read_json_object(path) {
        Ok(map) => match map.get("mcpServers").and_then(|v| v.get("topos")) {
            Some(entry) if is_topos_mcp_entry(entry) => State::Active,
            Some(_) => State::Stale,
            None => State::Absent,
        },
        Err(_) => State::Stale,
    }
}

/// Merge `mcpServers.topos` into a JSON config. Returns `Ok(true)` if a write
/// happened (the entry was missing or pointed elsewhere).
pub(crate) fn set_mcp_entry(path: &Path) -> Result<bool, String> {
    if json_mcp_state(path) == State::Active {
        return Ok(false);
    }
    let mut map = read_json_object(path)?;
    let servers = map
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers_map) = servers else {
        return Err(format!("{} mcpServers must be an object", path.display()));
    };
    servers_map.insert("topos".to_string(), mcp_entry());
    write_json_object(path, &map, true)?;
    Ok(true)
}

/// Remove `mcpServers.topos`. The key name is the ownership marker, so any
/// value under it is treated as Topos-owned even if stale.
pub(crate) fn remove_mcp_entry(path: &Path, dry_run: bool) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let mut map = read_json_object(path)?;
    let removed = match map.get_mut("mcpServers") {
        Some(Value::Object(servers)) => {
            let removed = servers.remove("topos").is_some();
            if removed && servers.is_empty() {
                map.remove("mcpServers");
            }
            removed
        }
        _ => false,
    };
    if removed && !dry_run {
        write_json_object(path, &map, true)?;
    }
    Ok(removed)
}

pub(crate) fn toml_mcp_state(path: &Path) -> State {
    let Ok(text) = fs::read_to_string(path) else {
        return State::Absent;
    };
    let Ok(doc) = text.parse::<DocumentMut>() else {
        return State::Stale;
    };
    let Some(servers) = doc.get("mcp_servers").and_then(Item::as_table) else {
        return State::Absent;
    };
    let Some(entry) = servers.get("topos") else {
        return State::Absent;
    };
    let Some(table) = entry.as_table() else {
        return State::Stale;
    };
    let command_ok = table
        .get("command")
        .and_then(Item::as_str)
        .is_some_and(is_topos_binary);
    let args_ok = table
        .get("args")
        .and_then(Item::as_array)
        .is_some_and(|args| args.iter().filter_map(|v| v.as_str()).eq(["mcp"]));
    if command_ok && args_ok {
        State::Active
    } else {
        State::Stale
    }
}

pub(crate) fn set_toml_mcp_entry(path: &Path) -> Result<bool, String> {
    if toml_mcp_state(path) == State::Active {
        return Ok(false);
    }
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|e| format!("parsing {}: {e}", path.display()))?;
    if doc.get("mcp_servers").and_then(Item::as_table).is_none() {
        doc["mcp_servers"] = Item::Table(Table::new());
    }
    let servers = doc["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| format!("{} mcp_servers must be a table", path.display()))?;
    let mut entry = Table::new();
    entry["command"] = value(topos_command());
    let mut args = Array::new();
    args.push("mcp");
    entry["args"] = value(args);
    servers.insert("topos", Item::Table(entry));
    atomic_write(path, &doc.to_string(), path.is_file())?;
    Ok(true)
}

pub(crate) fn remove_toml_mcp_entry(path: &Path, dry_run: bool) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|e| format!("parsing {}: {e}", path.display()))?;
    let removed = match doc.get_mut("mcp_servers").and_then(Item::as_table_mut) {
        Some(servers) => {
            let removed = servers.remove("topos").is_some();
            if removed && servers.is_empty() {
                doc.remove("mcp_servers");
            }
            removed
        }
        None => false,
    };
    if removed && !dry_run {
        atomic_write(path, &doc.to_string(), true)?;
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Marker-delimited Markdown block
// ---------------------------------------------------------------------------

const MARKER_START: &str = "<!-- topos:start -->";
const MARKER_END: &str = "<!-- topos:end -->";

fn marker_block() -> String {
    format!(
        "{MARKER_START}\nTopos is available for structural code-quality checks: run \
`topos evaluate <path> -r` or `topos inspect <file>` before committing significant \
changes. See `topos --help`.\n{MARKER_END}\n"
    )
}

pub(crate) fn marker_state(path: &Path) -> State {
    let Ok(text) = fs::read_to_string(path) else {
        return State::Absent;
    };
    match (text.contains(MARKER_START), text.contains(MARKER_END)) {
        (true, true) => State::Active,
        (false, false) => State::Absent,
        _ => State::Stale,
    }
}

pub(crate) fn set_marker_block(path: &Path) -> Result<bool, String> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    if existing.contains(MARKER_START) {
        return Ok(false);
    }
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let updated = format!("{existing}{separator}\n{}", marker_block());
    atomic_write(path, &updated, true)?;
    Ok(true)
}

pub(crate) fn remove_marker_block(path: &Path, dry_run: bool) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let (Some(start), Some(end_marker)) = (text.find(MARKER_START), text.find(MARKER_END)) else {
        return Ok(false);
    };
    let before = text[..start].trim_end();
    let after = text[end_marker + MARKER_END.len()..].trim_start();
    let mut updated = before.to_string();
    if !before.is_empty() && !after.is_empty() {
        updated.push_str("\n\n");
    }
    updated.push_str(after);
    if !dry_run {
        // A file trimmed down to nothing is only deleted by
        // `delete_text_if_blank_and_owned`, which checks that install created
        // it; otherwise the now-blank file stays where the user put it.
        atomic_write(path, &(updated.trim_end().to_string() + "\n"), true)?;
    }
    Ok(true)
}

// ---------------------------------------------------------------------------
// Written skill files
// ---------------------------------------------------------------------------

pub(crate) fn skill_state(path: &Path) -> State {
    match fs::read_to_string(path) {
        Ok(text) if text == SKILL_MD => State::Active,
        Ok(_) => State::Stale,
        Err(_) => State::Absent,
    }
}

pub(crate) fn write_skill(path: &Path) -> Result<bool, String> {
    if fs::read_to_string(path).ok().as_deref() == Some(SKILL_MD) {
        return Ok(false);
    }
    atomic_write(path, SKILL_MD, path.is_file())?;
    Ok(true)
}

/// Delete a file this installer wrote, but only while its content still
/// matches exactly what was written. A locally edited copy is reported as
/// [`Outcome::Preserved`] rather than clobbered.
pub(crate) fn remove_written_file(
    path: &Path,
    expected: &str,
    dry_run: bool,
) -> Result<Outcome, String> {
    match fs::read_to_string(path) {
        Ok(text) if text == expected => {
            if !dry_run {
                fs::remove_file(path).map_err(|e| format!("removing {}: {e}", path.display()))?;
            }
            Ok(Outcome::Changed)
        }
        Ok(_) => Ok(Outcome::Preserved),
        Err(_) => Ok(Outcome::Unchanged),
    }
}

// ---------------------------------------------------------------------------
// `@import` line plus its pointer file
// ---------------------------------------------------------------------------

fn import_line(pointer: &Path) -> String {
    format!("@import {}", pointer.display())
}

fn has_import_line(host: &Path, pointer: &Path) -> bool {
    let wanted = import_line(pointer);
    fs::read_to_string(host)
        .map(|text| text.lines().any(|line| line.trim() == wanted))
        .unwrap_or(false)
}

pub(crate) fn import_state(host: &Path, pointer: &Path) -> State {
    let line = if has_import_line(host, pointer) {
        State::Active
    } else {
        State::Absent
    };
    combine(line, skill_state(pointer))
}

pub(crate) fn set_import_line(host: &Path, pointer: &Path) -> Result<bool, String> {
    let pointer_changed = write_skill(pointer)?;
    if has_import_line(host, pointer) {
        return Ok(pointer_changed);
    }
    let existing = fs::read_to_string(host).unwrap_or_default();
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let updated = format!("{existing}{separator}{}\n", import_line(pointer));
    atomic_write(host, &updated, true)?;
    Ok(true)
}

pub(crate) fn remove_import_line(
    host: &Path,
    pointer: &Path,
    home: &Path,
    harness: &str,
    dry_run: bool,
) -> Result<Outcome, String> {
    let wanted = import_line(pointer);
    let mut removed = false;
    if let Ok(text) = fs::read_to_string(host) {
        if text.lines().any(|line| line.trim() == wanted) {
            removed = true;
            if !dry_run {
                let kept: Vec<&str> = text.lines().filter(|line| line.trim() != wanted).collect();
                atomic_write(host, &(kept.join("\n").trim_end().to_string() + "\n"), true)?;
                delete_text_if_blank_and_owned(host, home, harness, dry_run)?;
            }
        }
    }
    match remove_written_file(pointer, SKILL_MD, dry_run)? {
        // A locally edited pointer is kept, and says so, even though the
        // import line itself is gone.
        Outcome::Preserved => Ok(Outcome::Preserved),
        _ if removed => Ok(Outcome::Changed),
        outcome => Ok(outcome),
    }
}

// ---------------------------------------------------------------------------
// Ownership tracking
// ---------------------------------------------------------------------------

fn state_file_path(home: &Path) -> PathBuf {
    match std::env::var_os("XDG_STATE_HOME").filter(|base| !base.is_empty()) {
        Some(base) => PathBuf::from(base).join("topos/install.json"),
        None => home.join(".local/state/topos/install.json"),
    }
}

/// Record that `topos install` created `path` from scratch (it didn't exist
/// before), so uninstall knows it's safe to delete once emptied back out.
pub(crate) fn record_created_file(home: &Path, harness: &str, path: &Path) -> Result<(), String> {
    let state_path = state_file_path(home);
    let mut state = read_json_object(&state_path).unwrap_or_default();
    let entry = state
        .entry(harness.to_string())
        .or_insert_with(|| json!({ "createdFiles": [] }));
    if let Value::Object(entry_map) = entry {
        let list = entry_map
            .entry("createdFiles".to_string())
            .or_insert_with(|| Value::Array(Vec::new()));
        if let Value::Array(arr) = list {
            let value = Value::String(path.display().to_string());
            if !arr.contains(&value) {
                arr.push(value);
            }
        }
    }
    write_json_object(&state_path, &state, false)
}

pub(crate) fn was_created_by_install(home: &Path, harness: &str, path: &Path) -> bool {
    let target = path.display().to_string();
    read_json_object(&state_file_path(home))
        .unwrap_or_default()
        .get(harness)
        .and_then(|v| v.get("createdFiles"))
        .and_then(Value::as_array)
        .map(|arr| arr.iter().any(|v| v.as_str() == Some(target.as_str())))
        .unwrap_or(false)
}

pub(crate) fn clear_created_files(home: &Path, harness: &str) -> Result<(), String> {
    let state_path = state_file_path(home);
    let mut state = read_json_object(&state_path).unwrap_or_default();
    if state.remove(harness).is_none() {
        return Ok(());
    }
    if state.is_empty() {
        if state_path.is_file() {
            fs::remove_file(&state_path)
                .map_err(|e| format!("removing {}: {e}", state_path.display()))?;
        }
        Ok(())
    } else {
        write_json_object(&state_path, &state, false)
    }
}

/// Delete a JSON config entirely if `topos install` created it and it has
/// since been emptied back out by uninstall.
pub(crate) fn delete_if_empty_and_owned(
    path: &Path,
    home: &Path,
    harness: &str,
    dry_run: bool,
) -> Result<bool, String> {
    if dry_run || !path.is_file() || !was_created_by_install(home, harness, path) {
        return Ok(false);
    }
    if !read_json_object(path).unwrap_or_default().is_empty() {
        return Ok(false);
    }
    fs::remove_file(path).map_err(|e| format!("removing {}: {e}", path.display()))?;
    Ok(true)
}

/// Delete a text config entirely if `topos install` created it and its
/// content has since been trimmed down to nothing.
pub(crate) fn delete_text_if_blank_and_owned(
    path: &Path,
    home: &Path,
    harness: &str,
    dry_run: bool,
) -> Result<bool, String> {
    if dry_run || !path.is_file() || !was_created_by_install(home, harness, path) {
        return Ok(false);
    }
    if !fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return Ok(false);
    }
    fs::remove_file(path).map_err(|e| format!("removing {}: {e}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos-install-{label}-{}-{}",
            std::process::id(),
            label.len()
        ));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn json_mcp_entry_round_trips_through_install_and_uninstall() {
        let dir = tmp_dir("json-roundtrip");
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"mcpServers": {"other": {"command": "foo"}}}"#).unwrap();

        assert_eq!(json_mcp_state(&path), State::Absent);
        assert!(set_mcp_entry(&path).unwrap());
        assert_eq!(json_mcp_state(&path), State::Active);
        // Re-applying is a no-op, matching install's idempotency contract.
        assert!(!set_mcp_entry(&path).unwrap());

        assert!(remove_mcp_entry(&path, false).unwrap());
        assert_eq!(json_mcp_state(&path), State::Absent);
        // The foreign sibling entry must survive removal.
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"other\""));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn dry_run_uninstall_makes_no_filesystem_changes() {
        let dir = tmp_dir("dry-run");
        let path = dir.join("settings.json");
        set_mcp_entry(&path).unwrap();
        let before = fs::read_to_string(&path).unwrap();

        assert!(remove_mcp_entry(&path, true).unwrap());

        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(before, after, "dry-run must not touch the file");
        assert_eq!(json_mcp_state(&path), State::Active);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn stale_entry_is_detected_and_still_removable() {
        let dir = tmp_dir("stale");
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{"mcpServers": {"topos": {"command": "old-binary"}}}"#,
        )
        .unwrap();

        assert_eq!(json_mcp_state(&path), State::Stale);
        assert!(remove_mcp_entry(&path, false).unwrap());
        assert_eq!(json_mcp_state(&path), State::Absent);
        fs::remove_dir_all(dir).ok();
    }

    /// A `topos` binary somewhere else on disk — what `claude mcp add` or an
    /// install from a different prefix leaves behind — is already active, so
    /// re-installing neither rewrites the file nor reports it stale.
    #[test]
    fn a_topos_entry_from_another_location_counts_as_active() {
        let dir = tmp_dir("foreign-prefix");
        let path = dir.join("settings.json");
        fs::write(
            &path,
            r#"{"mcpServers": {"topos": {"command": "/opt/homebrew/bin/topos", "args": ["mcp"]}}}"#,
        )
        .unwrap();

        assert_eq!(json_mcp_state(&path), State::Active);
        assert!(!set_mcp_entry(&path).unwrap());
        fs::remove_dir_all(dir).ok();
    }

    /// Harnesses launched by the desktop environment have no shell `PATH`, so
    /// a bare command name would never resolve.
    #[test]
    fn installed_mcp_command_is_an_absolute_path() {
        let dir = tmp_dir("absolute-command");
        let path = dir.join("settings.json");
        set_mcp_entry(&path).unwrap();

        let map = read_json_object(&path).unwrap();
        let command = map["mcpServers"]["topos"]["command"].as_str().unwrap();
        assert!(
            Path::new(command).is_absolute(),
            "expected an absolute command, got {command}"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_private_config_keeps_its_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tmp_dir("permissions");
        let path = dir.join("claude.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        set_mcp_entry(&path).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "install must not widen a 0600 config");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_first_backup_is_the_pre_topos_original() {
        let dir = tmp_dir("backup-original");
        let path = dir.join("settings.json");
        fs::write(&path, r#"{"original": true}"#).unwrap();

        set_mcp_entry(&path).unwrap();
        // A later write (here: repairing a stale entry) must not replace the
        // snapshot with already-modified content.
        fs::write(&path, r#"{"mcpServers": {"topos": {"command": "stale"}}}"#).unwrap();
        set_mcp_entry(&path).unwrap();

        let backup = fs::read_to_string(backup_path(&path)).unwrap();
        assert!(backup.contains("\"original\""), "got backup: {backup}");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn toml_mcp_entry_round_trips_and_preserves_unrelated_tables() {
        let dir = tmp_dir("codex");
        let path = dir.join("config.toml");
        fs::write(&path, "[model]\nname = \"gpt\"\n").unwrap();

        assert_eq!(toml_mcp_state(&path), State::Absent);
        assert!(set_toml_mcp_entry(&path).unwrap());
        assert_eq!(toml_mcp_state(&path), State::Active);
        assert!(!set_toml_mcp_entry(&path).unwrap());

        assert!(remove_toml_mcp_entry(&path, false).unwrap());
        assert_eq!(toml_mcp_state(&path), State::Absent);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("name = \"gpt\""));
        assert!(!text.contains("mcp_servers"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn marker_block_is_delimited_and_leaves_other_content_alone() {
        let dir = tmp_dir("copilot");
        let path = dir.join("copilot-instructions.md");
        fs::write(&path, "# My instructions\nAlways use tabs.\n").unwrap();

        assert!(set_marker_block(&path).unwrap());
        assert_eq!(marker_state(&path), State::Active);

        assert!(remove_marker_block(&path, false).unwrap());
        assert_eq!(marker_state(&path), State::Absent);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("Always use tabs."));
        assert!(!text.contains("topos:start"));
        fs::remove_dir_all(dir).ok();
    }

    /// A file Topos did not create is kept even once our block was its only
    /// content — deleting it is `delete_text_if_blank_and_owned`'s call, and
    /// that only fires for files install created.
    #[test]
    fn an_unowned_file_survives_losing_its_last_block() {
        let dir = tmp_dir("unowned-blank");
        let path = dir.join("copilot-instructions.md");
        fs::write(&path, "").unwrap();
        set_marker_block(&path).unwrap();

        remove_marker_block(&path, false).unwrap();
        delete_text_if_blank_and_owned(&path, &dir, "copilot", false).unwrap();

        assert!(path.is_file(), "an unowned file must not be deleted");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn written_file_is_only_removed_when_content_still_matches() {
        let dir = tmp_dir("owned-file");
        let path = dir.join("SKILL.md");

        // A user edit is reported as preserved rather than clobbered.
        fs::write(&path, "user edited this").unwrap();
        assert_eq!(
            remove_written_file(&path, "expected", false).unwrap(),
            Outcome::Preserved
        );
        assert!(path.is_file());

        fs::write(&path, "expected").unwrap();
        assert_eq!(
            remove_written_file(&path, "expected", false).unwrap(),
            Outcome::Changed
        );
        assert!(!path.is_file());

        assert_eq!(
            remove_written_file(&path, "expected", false).unwrap(),
            Outcome::Unchanged
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn created_file_ownership_is_tracked_and_cleared() {
        let home = tmp_dir("ownership-home");
        let target = home.join("some/config.json");

        assert!(!was_created_by_install(&home, "claude", &target));
        record_created_file(&home, "claude", &target).unwrap();
        assert!(was_created_by_install(&home, "claude", &target));

        clear_created_files(&home, "claude").unwrap();
        assert!(!was_created_by_install(&home, "claude", &target));
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn import_line_round_trips() {
        let home = tmp_dir("antigravity");
        let host = home.join(".gemini/GEMINI.md");
        let pointer = home.join(".gemini/topos-skill.md");
        fs::create_dir_all(home.join(".gemini")).unwrap();
        fs::write(&host, "# My rules\nBe concise.\n").unwrap();

        assert_eq!(import_state(&host, &pointer), State::Absent);
        assert!(set_import_line(&host, &pointer).unwrap());
        assert_eq!(import_state(&host, &pointer), State::Active);

        assert_eq!(
            remove_import_line(&host, &pointer, &home, "antigravity", false).unwrap(),
            Outcome::Changed
        );
        assert_eq!(import_state(&host, &pointer), State::Absent);
        let text = fs::read_to_string(&host).unwrap();
        assert!(text.contains("Be concise."));
        fs::remove_dir_all(home).ok();
    }

    /// An edited pointer file is kept and reported as such, rather than being
    /// deleted along with the import line that referenced it.
    #[test]
    fn an_edited_pointer_file_is_preserved() {
        let home = tmp_dir("antigravity-edited");
        let host = home.join(".gemini/GEMINI.md");
        let pointer = home.join(".gemini/topos-skill.md");
        fs::create_dir_all(home.join(".gemini")).unwrap();
        set_import_line(&host, &pointer).unwrap();
        fs::write(&pointer, "locally rewritten").unwrap();

        assert_eq!(
            remove_import_line(&host, &pointer, &home, "antigravity", false).unwrap(),
            Outcome::Preserved
        );
        assert!(pointer.is_file(), "an edited pointer must survive");
        fs::remove_dir_all(home).ok();
    }

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
}
