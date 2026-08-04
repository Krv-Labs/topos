//! Which config files `topos install` created from scratch.
//!
//! Split out of `integrations.rs` alongside `edits.rs`. Editing a file Topos
//! shares with the harness is safe to do by marker — the `topos` key, the
//! `<!-- topos:start -->` block — but *deleting* a file is not, so uninstall
//! only removes one it has a record of creating. A pre-existing config that
//! happens to end up empty is left where the user put it.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::edits::{read_json_object, write_json_object};

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
    delete_if_owned(path, home, harness, dry_run, |path| {
        read_json_object(path).unwrap_or_default().is_empty()
    })
}

/// Delete a text config entirely if `topos install` created it and its
/// content has since been trimmed down to nothing.
pub(crate) fn delete_text_if_blank_and_owned(
    path: &Path,
    home: &Path,
    harness: &str,
    dry_run: bool,
) -> Result<bool, String> {
    delete_if_owned(path, home, harness, dry_run, |path| {
        fs::read_to_string(path)
            .unwrap_or_default()
            .trim()
            .is_empty()
    })
}

fn delete_if_owned(
    path: &Path,
    home: &Path,
    harness: &str,
    dry_run: bool,
    is_empty: impl FnOnce(&Path) -> bool,
) -> Result<bool, String> {
    if dry_run || !path.is_file() || !was_created_by_install(home, harness, path) {
        return Ok(false);
    }
    if !is_empty(path) {
        return Ok(false);
    }
    fs::remove_file(path).map_err(|e| format!("removing {}: {e}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::install::edits::{remove_marker_block, set_marker_block};
    use crate::commands::install::testing::tmp_dir;

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

    /// A file Topos did not create is kept even once our block was its only
    /// content.
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

    /// The same file, once install has recorded creating it, is cleaned up.
    #[test]
    fn an_owned_file_is_deleted_once_it_is_blank() {
        let dir = tmp_dir("owned-blank");
        let path = dir.join("copilot-instructions.md");
        set_marker_block(&path).unwrap();
        record_created_file(&dir, "copilot", &path).unwrap();

        remove_marker_block(&path, false).unwrap();
        assert!(delete_text_if_blank_and_owned(&path, &dir, "copilot", false).unwrap());

        assert!(!path.exists());
        fs::remove_dir_all(dir).ok();
    }
}
