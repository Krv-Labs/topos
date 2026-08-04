//! The read-modify-write primitives every harness adapter is built from, one
//! pair per config format: detect the current state, apply or remove Topos's
//! own entry, and leave everything else in the file untouched.
//!
//! Split out of `integrations.rs` -- editing files is a separate concern from
//! the harness table that decides which files to edit, and bundling both put
//! that module well past the SIMPLE gate (the same split `render.rs` made out
//! of `evaluate.rs`).
//!
//! Every write goes through [`atomic_write`], so all of them are crash-safe,
//! permission-preserving, and backed up identically.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use toml_edit::{value, Array, DocumentMut, Item, Table};

use super::integrations::{combine, Outcome, State};
use super::ownership::delete_text_if_blank_and_owned;

/// Embedded so a globally-installed `topos` binary (cargo install, the curl
/// installer, a package manager) can drop the skill file without a checkout
/// of this repository on disk.
pub(crate) const SKILL_MD: &str = include_str!("../../../../../skills/topos/SKILL.md");

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
// JSON documents
// ---------------------------------------------------------------------------

/// Read a JSON object config, treating a missing file as empty. Errors on
/// unreadable/invalid content or a non-object top level so callers never
/// silently clobber something they can't safely parse.
pub(crate) fn read_json_object(path: &Path) -> Result<Map<String, Value>, String> {
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

pub(crate) fn write_json_object(
    path: &Path,
    data: &Map<String, Value>,
    backup: bool,
) -> Result<(), String> {
    let contents = serde_json::to_string_pretty(data).map_err(|e| e.to_string())? + "\n";
    atomic_write(path, &contents, backup)
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

// ---------------------------------------------------------------------------
// TOML documents (`toml_edit` preserves comments and formatting)
// ---------------------------------------------------------------------------

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::install::testing::tmp_dir;

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
}
