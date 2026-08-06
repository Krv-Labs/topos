//! Ownership state so uninstall only ever removes what install created.
//!
//! Two questions have to survive between the two commands, and the filesystem
//! cannot answer either of them at uninstall time:
//!
//! * **Did topos create this config file, or was it already there?** A
//!   `~/.cursor/mcp.json` left holding nothing but `{}` looks the same whether
//!   the user made it or we did, and deleting the user's file is a trace in the
//!   wrong direction.
//! * **Which directories did topos have to create?** `~/.copilot` may predate
//!   us or may not, and only the write that brought it into existence knows.
//!
//! Schema (the draft's flat `{"<id>": {...}}` shape is tolerated and upgraded
//! on read):
//!
//! ```json
//! { "harnesses": { "<id>": { "createdFiles": [...] } }, "createdDirs": [...] }
//! ```
//!
//! `install.json` is topos's own file, which makes it special twice over: its
//! writes always pass `backup = false`, because there is never user content to
//! preserve, and the directories those writes create are deliberately *not* fed
//! back into [`record_created_dirs`] — the ledger lives inside a prunable
//! directory, so recording its own parents would be self-referential. Uninstall
//! prunes [`state_dir`] by name, last of all.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::fsops::{read_json_object, write_json_object};
use super::paths;

/// Top-level key holding the per-harness records.
const HARNESSES_KEY: &str = "harnesses";
/// Key, inside one harness record, listing the files install created.
const CREATED_FILES_KEY: &str = "createdFiles";
/// Top-level key listing every directory install created.
const CREATED_DIRS_KEY: &str = "createdDirs";
/// The ledger's file name inside [`state_dir`].
const STATE_FILE: &str = "install.json";

/// The ledger in memory, split into the two lists that are read independently.
///
/// They are kept apart rather than as one `Value` because uninstall reads
/// `dirs` after it has cleared every harness: burying the directory list inside
/// the harness map is exactly the mistake that loses it mid-uninstall.
struct Ledger {
    /// Harness id → `{"createdFiles": [...]}`.
    harnesses: Map<String, Value>,
    /// Created directories, as the strings they are stored as.
    dirs: Vec<String>,
}

/// Directory holding `install.json` — `~/.local/state/topos`, or
/// `%APPDATA%\topos` on Windows. Pruned last, after `install.json` itself.
pub(crate) fn state_dir(home: &Path) -> PathBuf {
    if cfg!(windows) {
        paths::app_data(home).join("topos")
    } else {
        // XDG *state*, not config: this is machine-local bookkeeping nobody
        // would want synced between hosts.
        home.join(".local").join("state").join("topos")
    }
}

pub(crate) fn state_file_path(home: &Path) -> PathBuf {
    state_dir(home).join(STATE_FILE)
}

pub(crate) fn record_created_file(home: &Path, harness: &str, path: &Path) -> Result<(), String> {
    let mut ledger = load(home);
    let mut files = files_of(&ledger.harnesses, harness);
    let key = path.display().to_string();
    // Install is re-runnable, so the same file arrives here repeatedly.
    if !files.contains(&key) {
        files.push(key);
    }
    let mut record = Map::new();
    record.insert(
        CREATED_FILES_KEY.to_string(),
        files.into_iter().map(Value::String).collect(),
    );
    ledger
        .harnesses
        .insert(harness.to_string(), Value::Object(record));
    save(home, &ledger)
}

pub(crate) fn was_created_by_install(home: &Path, harness: &str, path: &Path) -> bool {
    files_of(&load(home).harnesses, harness).contains(&path.display().to_string())
}

/// Merge `dirs` into `createdDirs`, skipping duplicates and any path in
/// `NEVER_PRUNE`.
pub(crate) fn record_created_dirs(home: &Path, dirs: &[PathBuf]) -> Result<(), String> {
    let shared = never_prune(home);
    let fresh: Vec<String> = dirs
        .iter()
        // Membership is tested on `Path`, which compares component-wise and
        // accepts either separator on Windows, rather than on the stored
        // string, where `~/.local/state` and `~\.local\state` would differ.
        .filter(|dir| !shared.contains(dir))
        .map(|dir| dir.display().to_string())
        .collect();
    // Nothing to say, nothing to write: an uninstall on a machine that never
    // installed must not bring the ledger — and `~/.local` with it — into
    // existence just to record an empty list.
    if fresh.is_empty() {
        return Ok(());
    }
    let mut ledger = load(home);
    ledger.dirs.extend(fresh);
    // De-duplicates against what is already recorded *and* within this call,
    // keeping first-seen order so the list stays shallowest-first.
    let mut seen = std::collections::HashSet::new();
    ledger.dirs.retain(|dir| seen.insert(dir.clone()));
    save(home, &ledger)
}

/// Every recorded created directory. **Callers must read this before deleting
/// `install.json`** — the state file lives inside a prunable directory.
pub(crate) fn created_dirs(home: &Path) -> Vec<PathBuf> {
    load(home).dirs.into_iter().map(PathBuf::from).collect()
}

/// Forget every file recorded for `harness`.
///
/// Unlike the draft, this keeps the document on disk when the last harness goes
/// away: `createdDirs` still has to survive until uninstall has finished
/// pruning. Deleting the file is [`remove_state_file`]'s job. The early return
/// is the same restraint as in [`record_created_dirs`] — recording an absence
/// is not worth creating a ledger for.
pub(crate) fn clear_created_files(home: &Path, harness: &str) -> Result<(), String> {
    if !state_file_path(home).exists() {
        return Ok(());
    }
    let mut ledger = load(home);
    ledger.harnesses.remove(harness);
    save(home, &ledger)
}

/// Delete `install.json` if it exists. Missing is not an error.
pub(crate) fn remove_state_file(home: &Path) -> Result<(), String> {
    let path = state_file_path(home);
    if !path.exists() {
        return Ok(());
    }
    fs::remove_file(&path).map_err(|e| format!("removing {}: {e}", path.display()))
}

/// Shared directories that are never candidates for pruning even if topos
/// created them: `$HOME` itself, `~/.local`, `~/.local/state`, `~/.config`,
/// `~/Library`, `~/Library/Application Support`, and `%APPDATA%`.
///
/// Being the process that happened to create `~/.local/state` does not make it
/// ours: the next tool to want it would find it gone. `%APPDATA%` is listed
/// unconditionally because the cost of an unreachable entry on Unix is nothing
/// next to the cost of a missing one on Windows.
fn never_prune(home: &Path) -> Vec<PathBuf> {
    vec![
        home.to_path_buf(),
        home.join(".local"),
        home.join(".local").join("state"),
        home.join(".config"),
        home.join("Library"),
        home.join("Library").join("Application Support"),
        paths::app_data(home),
    ]
}

/// The ledger, in the current shape whatever shape it was written in.
///
/// Everything that is not `createdDirs` and not the `harnesses` wrapper is read
/// as the draft's flat `{"<id>": {"createdFiles": [...]}}` map: anyone who ran
/// the previous build of this branch has that on disk right now, and it is the
/// only record of which config files uninstall may delete. `createdDirs` is
/// lifted out *first*, so the upgrade cannot bury the directory list under
/// `harnesses` where [`created_dirs`] would no longer see it.
///
/// An unreadable or unparseable ledger reads as empty rather than as an error.
/// [`created_dirs`] has no error channel, so the two would otherwise disagree,
/// and a corrupt bookkeeping file is no reason to refuse to install — the next
/// write replaces it.
///
/// The local is `stored`, never `raw`: tree-sitter's Rust grammar reads a
/// `&raw` argument as the raw-reference syntax and marks the whole file
/// unparseable.
fn load(home: &Path) -> Ledger {
    let mut stored = read_json_object(&state_file_path(home)).unwrap_or_default();
    let dirs = strings(&stored, CREATED_DIRS_KEY);
    stored.remove(CREATED_DIRS_KEY);
    let harnesses = match stored.remove(HARNESSES_KEY) {
        Some(Value::Object(nested)) => nested,
        _ => stored,
    };
    Ledger { harnesses, dirs }
}

/// Persist the ledger, always in the current shape.
///
/// `backup = false` and the returned `created_dirs` are dropped on purpose —
/// see the module docs.
fn save(home: &Path, ledger: &Ledger) -> Result<(), String> {
    let mut doc = Map::new();
    doc.insert(
        HARNESSES_KEY.to_string(),
        Value::Object(ledger.harnesses.clone()),
    );
    doc.insert(
        CREATED_DIRS_KEY.to_string(),
        ledger.dirs.iter().cloned().map(Value::String).collect(),
    );
    write_json_object(&state_file_path(home), &doc, false).map(|_| ())
}

/// The files recorded for `harness`, or an empty list when it has none.
fn files_of(harnesses: &Map<String, Value>, harness: &str) -> Vec<String> {
    harnesses
        .get(harness)
        .and_then(Value::as_object)
        .map(|record| strings(record, CREATED_FILES_KEY))
        .unwrap_or_default()
}

/// The string members of the array at `key`, ignoring anything else that may
/// have been hand-edited in.
fn strings(map: &Map<String, Value>, key: &str) -> Vec<String> {
    map.get(key)
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).map(str::to_string))
        .map(Iterator::collect)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::install::testing::tmp_dir;

    /// Write a ledger verbatim, for the shapes an earlier build left behind.
    fn seed_ledger(home: &Path, text: &str) {
        let path = state_file_path(home);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
    }

    #[test]
    fn the_ledger_lives_in_the_platform_state_directory() {
        let home = Path::new("/scratch/home");
        let path = state_file_path(home);

        assert_eq!(path.parent().unwrap(), state_dir(home));
        assert!(path.ends_with("install.json"));
        if cfg!(windows) {
            assert_eq!(state_dir(home), paths::app_data(home).join("topos"));
        } else {
            assert_eq!(state_dir(home), home.join(".local/state/topos"));
        }
    }

    #[test]
    fn a_created_file_is_recorded_queried_and_cleared() {
        let home = tmp_dir("round-trip");
        let config = home.join(".cursor/mcp.json");

        assert!(!was_created_by_install(&home, "cursor", &config));
        record_created_file(&home, "cursor", &config).unwrap();
        assert!(was_created_by_install(&home, "cursor", &config));

        // Install is re-runnable: recording twice must not double the entry.
        record_created_file(&home, "cursor", &config).unwrap();
        assert_eq!(
            files_of(&load(&home).harnesses, "cursor"),
            vec![config.display().to_string()]
        );

        clear_created_files(&home, "cursor").unwrap();
        assert!(!was_created_by_install(&home, "cursor", &config));
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn a_file_recorded_for_one_harness_is_invisible_to_another() {
        let home = tmp_dir("isolation");
        let shared_config = home.join(".gemini/settings.json");
        let antigravity_config = home.join(".gemini/config/mcp_config.json");
        record_created_file(&home, "gemini", &shared_config).unwrap();
        record_created_file(&home, "antigravity", &antigravity_config).unwrap();

        assert!(was_created_by_install(&home, "gemini", &shared_config));
        assert!(!was_created_by_install(
            &home,
            "antigravity",
            &shared_config
        ));

        // Clearing one harness leaves the other's record alone.
        clear_created_files(&home, "gemini").unwrap();
        assert!(was_created_by_install(
            &home,
            "antigravity",
            &antigravity_config
        ));
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn created_directories_are_deduplicated_within_and_across_calls() {
        let home = tmp_dir("dedupe");
        let copilot = home.join(".copilot");

        record_created_dirs(&home, &[copilot.clone(), copilot.clone()]).unwrap();
        record_created_dirs(&home, &[copilot.clone(), home.join(".cursor")]).unwrap();

        assert_eq!(created_dirs(&home), vec![copilot, home.join(".cursor")]);
        fs::remove_dir_all(home).ok();
    }

    /// Pruning a shared XDG or OS directory because topos happened to create it
    /// would delete far outside topos's own scope.
    #[test]
    fn shared_directories_are_never_recorded_as_prunable() {
        let home = tmp_dir("never-prune");
        let ours = home.join(".local/state/topos");

        let mut dirs = never_prune(&home);
        dirs.push(ours.clone());
        record_created_dirs(&home, &dirs).unwrap();

        assert_eq!(created_dirs(&home), vec![ours]);
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn recording_only_shared_directories_does_not_create_the_ledger() {
        let home = tmp_dir("no-ledger");

        record_created_dirs(&home, &[home.join(".local"), home.clone()]).unwrap();

        assert!(!state_file_path(&home).exists(), "wrote an empty ledger");
        assert!(created_dirs(&home).is_empty());
        fs::remove_dir_all(home).ok();
    }

    /// Users who ran the previous build of this branch have the flat shape on
    /// disk, and it is the only record of what uninstall may delete.
    #[test]
    fn the_drafts_flat_schema_is_read_and_upgraded_in_place() {
        let home = tmp_dir("upgrade");
        let config = home.join(".claude.json");
        seed_ledger(
            &home,
            &format!(
                r#"{{"claude": {{"createdFiles": ["{}"]}}}}"#,
                config.display()
            ),
        );

        assert!(was_created_by_install(&home, "claude", &config));

        // The upgrade has to survive the next write, not just the read.
        record_created_file(&home, "codex", &home.join(".codex/config.toml")).unwrap();
        let stored = read_json_object(&state_file_path(&home)).unwrap();
        assert!(stored.contains_key(HARNESSES_KEY), "shape not upgraded");
        assert!(stored.get("claude").is_none(), "flat key survived");
        assert!(was_created_by_install(&home, "claude", &config));
        fs::remove_dir_all(home).ok();
    }

    /// `createdDirs` must not be buried under `harnesses` by the upgrade — that
    /// would silently lose the directory list mid-uninstall.
    #[test]
    fn an_upgrade_keeps_created_directories_where_they_can_be_read() {
        let home = tmp_dir("upgrade-dirs");
        seed_ledger(
            &home,
            &format!(
                r#"{{"createdDirs": ["{}"], "cursor": {{"createdFiles": []}}}}"#,
                home.join(".cursor").display()
            ),
        );

        assert_eq!(created_dirs(&home), vec![home.join(".cursor")]);
        fs::remove_dir_all(home).ok();
    }

    /// Uninstall clears each harness before it prunes, so the last clear must
    /// not take the directory list with it.
    #[test]
    fn clearing_the_last_harness_leaves_the_directory_list_readable() {
        let home = tmp_dir("last-harness");
        let created = home.join(".copilot");
        record_created_file(&home, "copilot", &created.join("mcp-config.json")).unwrap();
        record_created_dirs(&home, std::slice::from_ref(&created)).unwrap();

        clear_created_files(&home, "copilot").unwrap();

        assert!(state_file_path(&home).is_file(), "the ledger was deleted");
        assert_eq!(created_dirs(&home), vec![created]);
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn removing_the_ledger_is_idempotent_and_comes_after_reading_it() {
        let home = tmp_dir("remove");
        let created = home.join(".cursor");
        record_created_dirs(&home, std::slice::from_ref(&created)).unwrap();

        let dirs = created_dirs(&home);
        remove_state_file(&home).unwrap();

        assert_eq!(dirs, vec![created]);
        assert!(!state_file_path(&home).exists());
        // Missing is not an error: uninstall runs on machines that never
        // installed.
        remove_state_file(&home).unwrap();
        assert!(created_dirs(&home).is_empty());
        fs::remove_dir_all(home).ok();
    }

    /// The ledger is topos's own file: there is never user content to preserve,
    /// and a `.topos.backup` beside it would itself be a trace.
    #[test]
    fn the_ledger_is_never_backed_up_and_never_records_its_own_parents() {
        let home = tmp_dir("no-backup");
        record_created_file(&home, "claude", &home.join(".claude.json")).unwrap();
        record_created_file(&home, "claude", &home.join(".claude.json")).unwrap();

        let backup = super::super::fsops::backup_path(&state_file_path(&home));
        assert!(!backup.exists(), "backed up our own bookkeeping file");
        assert!(
            created_dirs(&home).is_empty(),
            "the ledger recorded the directories its own write created"
        );
        fs::remove_dir_all(home).ok();
    }

    /// A ledger someone truncated or hand-edited must not wedge install.
    #[test]
    fn an_unreadable_ledger_reads_as_empty_and_is_rewritten() {
        let home = tmp_dir("corrupt");
        let config = home.join(".claude.json");
        seed_ledger(&home, "{ not json");

        assert!(!was_created_by_install(&home, "claude", &config));
        assert!(created_dirs(&home).is_empty());
        record_created_file(&home, "claude", &config).unwrap();
        assert!(was_created_by_install(&home, "claude", &config));
        fs::remove_dir_all(home).ok();
    }

    #[test]
    fn clearing_a_harness_on_a_machine_that_never_installed_writes_nothing() {
        let home = tmp_dir("clear-clean");

        clear_created_files(&home, "claude").unwrap();

        assert!(!state_dir(&home).exists(), "uninstall created ~/.local");
        fs::remove_dir_all(home).ok();
    }
}
