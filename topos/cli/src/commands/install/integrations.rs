//! Per-harness config paths, state detection, and the small persistence
//! primitives `install`/`uninstall`/`status` build on: atomic writes with
//! backups, and a tiny ownership-tracking file so uninstall only ever
//! deletes what a previous `topos install` actually created.
//!
//! Ported from the harness-installer pattern in sgathrid/brian
//! (`wikicli/lifecycle/{integrations,install,uninstall}.py`): a flat
//! per-harness dispatch, marker-based idempotent edits, and an
//! install-state file recording exactly what was added so uninstall never
//! touches a setting it didn't create. Brian injects session-start context
//! via hooks; Topos exposes MCP tools instead, so the Topos adapters
//! register an MCP server (or, where a harness has no MCP support, drop a
//! marked instructions block / skill file) rather than porting the hook
//! machinery.
//!
//! Schema notes worth re-checking against each harness's current docs
//! before this ships out of draft:
//! - Claude Code stores user-scope MCP servers in `~/.claude.json`, not
//!   `~/.claude/settings.json` (settings.json is hooks/permissions only).
//! - Codex CLI, Gemini CLI, and Cursor accept the same
//!   `{"command": "topos", "args": ["mcp"]}` shape Claude Desktop uses.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use toml_edit::{value, Array, DocumentMut, Item, Table};

/// Embedded so a globally-installed `topos` binary (cargo install, the
/// curl installer, a package manager) can drop the skill file without a
/// checkout of this repository on disk.
pub(crate) const SKILL_MD: &str = include_str!("../../../../../skills/topos/SKILL.md");

pub(crate) const SUPPORTED: [&str; 7] = [
    "claude",
    "claude-desktop",
    "codex",
    "gemini",
    "copilot",
    "skills",
    "antigravity",
];

pub(crate) fn harness_name(id: &str) -> &'static str {
    match id {
        "claude" => "Claude Code",
        "claude-desktop" => "Claude Desktop App",
        "codex" => "Codex CLI",
        "gemini" => "Gemini CLI",
        "copilot" => "GitHub Copilot CLI",
        "skills" => "Cursor & VS Code",
        "antigravity" => "Google Antigravity",
        _ => "Unknown harness",
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum State {
    Active,
    Stale,
    Absent,
}

pub(crate) fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| "cannot resolve home directory (HOME is unset)".to_string())
}

pub(crate) fn claude_desktop_config_path(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Claude/claude_desktop_config.json")
    } else {
        // Claude Desktop is not currently distributed for Linux; keep the
        // conventional path so status/uninstall can clean up configs left
        // by an earlier install.
        home.join(".config/Claude/claude_desktop_config.json")
    }
}

pub(crate) fn skill_path_claude(home: &Path) -> PathBuf {
    home.join(".claude/skills/topos/SKILL.md")
}

pub(crate) fn skill_path_agents(home: &Path) -> PathBuf {
    home.join(".agents/skills/topos/SKILL.md")
}

/// Directory used by the interactive menu to pre-select "detected" harnesses
/// that aren't yet configured for Topos.
pub(crate) fn detect_dir(id: &str, home: &Path) -> PathBuf {
    match id {
        "claude" => home.join(".claude"),
        "claude-desktop" => claude_desktop_config_path(home)
            .parent()
            .expect("claude desktop config path always has a parent")
            .to_path_buf(),
        "codex" => home.join(".codex"),
        "gemini" | "antigravity" => home.join(".gemini"),
        "copilot" => home.join(".copilot"),
        "skills" => home.join(".agents/skills"),
        _ => home.to_path_buf(),
    }
}

fn mcp_entry() -> Value {
    json!({ "command": "topos", "args": ["mcp"] })
}

pub(crate) fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".topos.backup");
    path.with_file_name(name)
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".topos.tmp");
    path.with_file_name(name)
}

/// Write via a temp file + rename, optionally snapshotting the previous
/// contents first. Creates parent directories as needed.
fn atomic_write(path: &Path, contents: &str, backup: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    if backup && path.is_file() {
        fs::copy(path, backup_path(path))
            .map_err(|e| format!("backing up {}: {e}", path.display()))?;
    }
    let tmp = tmp_path(path);
    fs::write(&tmp, contents).map_err(|e| format!("writing {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("replacing {}: {e}", path.display()))
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

/// `Active` when `mcpServers.topos` matches exactly, `Stale` when the key
/// exists with different content (or the file fails to parse), `Absent`
/// otherwise.
pub(crate) fn json_mcp_state(path: &Path) -> State {
    match read_json_object(path) {
        Ok(map) => match map.get("mcpServers").and_then(|v| v.get("topos")) {
            Some(entry) if *entry == mcp_entry() => State::Active,
            Some(_) => State::Stale,
            None => State::Absent,
        },
        Err(_) => State::Stale,
    }
}

/// Merge `mcpServers.topos` into a JSON config. Returns `Ok(true)` if a
/// write happened (the entry was missing or different).
pub(crate) fn set_mcp_entry(path: &Path) -> Result<bool, String> {
    let mut map = read_json_object(path)?;
    let servers = map
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers_map) = servers else {
        return Err(format!("{} mcpServers must be an object", path.display()));
    };
    if servers_map.get("topos") == Some(&mcp_entry()) {
        return Ok(false);
    }
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

pub(crate) fn codex_state(path: &Path) -> State {
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
    let command = table.get("command").and_then(Item::as_str);
    let args_ok = table
        .get("args")
        .and_then(Item::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>() == vec!["mcp"])
        .unwrap_or(false);
    if command == Some("topos") && args_ok {
        State::Active
    } else {
        State::Stale
    }
}

pub(crate) fn set_codex_entry(path: &Path) -> Result<bool, String> {
    if codex_state(path) == State::Active {
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
    entry["command"] = value("topos");
    let mut args = Array::new();
    args.push("mcp");
    entry["args"] = value(args);
    servers.insert("topos", Item::Table(entry));
    atomic_write(path, &doc.to_string(), path.is_file())?;
    Ok(true)
}

pub(crate) fn remove_codex_entry(path: &Path, dry_run: bool) -> Result<bool, String> {
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

const COPILOT_START: &str = "<!-- topos:start -->";
const COPILOT_END: &str = "<!-- topos:end -->";

fn copilot_block() -> String {
    format!(
        "{COPILOT_START}\nTopos is available for structural code-quality checks: run \
`topos evaluate <path> -r` or `topos inspect <file>` before committing significant \
changes. See `topos --help`.\n{COPILOT_END}\n"
    )
}

pub(crate) fn copilot_state(path: &Path) -> State {
    let Ok(text) = fs::read_to_string(path) else {
        return State::Absent;
    };
    match (text.contains(COPILOT_START), text.contains(COPILOT_END)) {
        (true, true) => State::Active,
        (false, false) => State::Absent,
        _ => State::Stale,
    }
}

pub(crate) fn set_copilot_block(path: &Path) -> Result<bool, String> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    if existing.contains(COPILOT_START) {
        return Ok(false);
    }
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let updated = format!("{existing}{separator}\n{}", copilot_block());
    atomic_write(path, &updated, true)?;
    Ok(true)
}

pub(crate) fn remove_copilot_block(path: &Path, dry_run: bool) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let (Some(start), Some(end_marker)) = (text.find(COPILOT_START), text.find(COPILOT_END)) else {
        return Ok(false);
    };
    let end = end_marker + COPILOT_END.len();
    let before = text[..start].trim_end();
    let after = text[end..].trim_start();
    let mut updated = before.to_string();
    if !before.is_empty() && !after.is_empty() {
        updated.push_str("\n\n");
    }
    updated.push_str(after);
    if !dry_run {
        if updated.trim().is_empty() {
            fs::remove_file(path).map_err(|e| format!("removing {}: {e}", path.display()))?;
        } else {
            atomic_write(path, &(updated.trim_end().to_string() + "\n"), true)?;
        }
    }
    Ok(true)
}

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

/// Remove a file this installer wrote only if its content still matches
/// exactly what was written — a user edit is left in place, reported by the
/// caller as preserved rather than silently overwritten or deleted.
pub(crate) fn remove_owned_file(
    path: &Path,
    expected: &str,
    dry_run: bool,
) -> Result<bool, String> {
    match fs::read_to_string(path) {
        Ok(text) if text == expected => {
            if !dry_run {
                fs::remove_file(path).map_err(|e| format!("removing {}: {e}", path.display()))?;
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

pub(crate) fn antigravity_pointer_path(home: &Path) -> PathBuf {
    home.join(".gemini/topos-skill.md")
}

fn antigravity_import_line(home: &Path) -> String {
    format!("@import {}", antigravity_pointer_path(home).display())
}

pub(crate) fn antigravity_state(home: &Path) -> State {
    let gemini_md = home.join(".gemini/GEMINI.md");
    let import_line = antigravity_import_line(home);
    let import_present = fs::read_to_string(&gemini_md)
        .map(|text| text.lines().any(|line| line.trim() == import_line))
        .unwrap_or(false);
    let pointer_present = antigravity_pointer_path(home).is_file();
    match (import_present, pointer_present) {
        (true, true) => State::Active,
        (false, false) => State::Absent,
        _ => State::Stale,
    }
}

pub(crate) fn set_antigravity_import(home: &Path) -> Result<bool, String> {
    let pointer = antigravity_pointer_path(home);
    let pointer_changed = fs::read_to_string(&pointer).ok().as_deref() != Some(SKILL_MD);
    if pointer_changed {
        atomic_write(&pointer, SKILL_MD, pointer.is_file())?;
    }
    let gemini_md = home.join(".gemini/GEMINI.md");
    let existing = fs::read_to_string(&gemini_md).unwrap_or_default();
    let import_line = antigravity_import_line(home);
    if existing.lines().any(|line| line.trim() == import_line) {
        return Ok(pointer_changed);
    }
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let updated = format!("{existing}{separator}{import_line}\n");
    atomic_write(&gemini_md, &updated, true)?;
    Ok(true)
}

pub(crate) fn remove_antigravity_import(home: &Path, dry_run: bool) -> Result<bool, String> {
    let gemini_md = home.join(".gemini/GEMINI.md");
    let import_line = antigravity_import_line(home);
    let mut changed = false;
    if let Ok(text) = fs::read_to_string(&gemini_md) {
        if text.lines().any(|line| line.trim() == import_line) {
            changed = true;
            if !dry_run {
                let kept: Vec<&str> = text
                    .lines()
                    .filter(|line| line.trim() != import_line)
                    .collect();
                if kept.iter().all(|line| line.trim().is_empty()) {
                    fs::remove_file(&gemini_md).ok();
                } else {
                    atomic_write(&gemini_md, &(kept.join("\n") + "\n"), true)?;
                }
            }
        }
    }
    let pointer = antigravity_pointer_path(home);
    if pointer.is_file() {
        changed = true;
        if !dry_run {
            fs::remove_file(&pointer)
                .map_err(|e| format!("removing {}: {e}", pointer.display()))?;
        }
    }
    Ok(changed)
}

fn combine(a: State, b: State) -> State {
    match (a, b) {
        (State::Active, State::Active) => State::Active,
        (State::Absent, State::Absent) => State::Absent,
        _ => State::Stale,
    }
}

/// Overall state for one harness, folding together every artifact it owns
/// (an MCP entry plus a skill file, for the two harnesses that get both).
pub(crate) fn integration_state(id: &str, home: &Path) -> State {
    match id {
        "claude" => combine(
            json_mcp_state(&home.join(".claude.json")),
            skill_state(&skill_path_claude(home)),
        ),
        "claude-desktop" => json_mcp_state(&claude_desktop_config_path(home)),
        "codex" => codex_state(&home.join(".codex/config.toml")),
        "gemini" => json_mcp_state(&home.join(".gemini/settings.json")),
        "copilot" => copilot_state(&home.join(".copilot/copilot-instructions.md")),
        "skills" => combine(
            json_mcp_state(&home.join(".cursor/mcp.json")),
            skill_state(&skill_path_agents(home)),
        ),
        "antigravity" => antigravity_state(home),
        _ => State::Absent,
    }
}

fn state_file_path(home: &Path) -> PathBuf {
    home.join(".local/state/topos/install.json")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "topos-install-{label}-{}-{}",
            std::process::id(),
            label.len()
        ));
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

    #[test]
    fn codex_toml_entry_round_trips_and_preserves_unrelated_tables() {
        let dir = tmp_dir("codex");
        let path = dir.join("config.toml");
        fs::write(&path, "[model]\nname = \"gpt\"\n").unwrap();

        assert_eq!(codex_state(&path), State::Absent);
        assert!(set_codex_entry(&path).unwrap());
        assert_eq!(codex_state(&path), State::Active);
        assert!(!set_codex_entry(&path).unwrap());

        assert!(remove_codex_entry(&path, false).unwrap());
        assert_eq!(codex_state(&path), State::Absent);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("name = \"gpt\""));
        assert!(!text.contains("mcp_servers"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn copilot_block_is_marker_delimited_and_leaves_other_content_alone() {
        let dir = tmp_dir("copilot");
        let path = dir.join("copilot-instructions.md");
        fs::write(&path, "# My instructions\nAlways use tabs.\n").unwrap();

        assert!(set_copilot_block(&path).unwrap());
        assert_eq!(copilot_state(&path), State::Active);

        assert!(remove_copilot_block(&path, false).unwrap());
        assert_eq!(copilot_state(&path), State::Absent);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("Always use tabs."));
        assert!(!text.contains("topos:start"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn owned_file_is_only_removed_when_content_still_matches() {
        let dir = tmp_dir("owned-file");
        let path = dir.join("SKILL.md");
        fs::write(&path, "expected").unwrap();

        // A user edit means the removal is skipped rather than clobbered.
        fs::write(&path, "user edited this").unwrap();
        assert!(!remove_owned_file(&path, "expected", false).unwrap());
        assert!(path.is_file());

        fs::write(&path, "expected").unwrap();
        assert!(remove_owned_file(&path, "expected", false).unwrap());
        assert!(!path.is_file());
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
    fn antigravity_import_round_trips() {
        let home = tmp_dir("antigravity");
        fs::create_dir_all(home.join(".gemini")).unwrap();
        fs::write(home.join(".gemini/GEMINI.md"), "# My rules\nBe concise.\n").unwrap();

        assert_eq!(antigravity_state(&home), State::Absent);
        assert!(set_antigravity_import(&home).unwrap());
        assert_eq!(antigravity_state(&home), State::Active);

        assert!(remove_antigravity_import(&home, false).unwrap());
        assert_eq!(antigravity_state(&home), State::Absent);
        let text = fs::read_to_string(home.join(".gemini/GEMINI.md")).unwrap();
        assert!(text.contains("Be concise."));
        fs::remove_dir_all(home).ok();
    }
}
