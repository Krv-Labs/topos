//! Reading, classifying, writing and removing `[mcp_servers.topos]` in Codex
//! CLI's `config.toml`.
//!
//! Edited through `toml_edit` rather than regex over raw text, so a user's
//! comments, key order and formatting survive an install/uninstall cycle
//! untouched.

use std::fs;
use std::path::Path;

use toml_edit::{value, Array, DocumentMut, Item, TableLike};

use super::artifact::{names_topos, points_at_topos, Inspection, State, MCP_ARGS, SERVER_KEY};
use super::binary::drift;
use super::fsops::{atomic_write, WriteOutcome};

const CONTAINER: &str = "mcp_servers";

pub(crate) fn inspect(path: &Path, binary: &Path) -> Inspection {
    let doc = match read(path) {
        Ok(Some(doc)) => doc,
        Ok(None) => return Inspection::plain(State::Absent),
        Err(message) => return Inspection::conflict(message),
    };
    let Some(entry) = entry_of(&doc) else {
        return Inspection::plain(State::Absent);
    };
    let command = entry
        .get("command")
        .and_then(Item::as_str)
        .unwrap_or_default();
    if !names_topos(command) || !args_are_mcp(entry) {
        return Inspection::conflict(format!(
            "[{CONTAINER}.{SERVER_KEY}] in {} is an entry topos did not write — inspect it by hand",
            path.display()
        ));
    }
    match drift(command, binary) {
        Some(reason) => Inspection::incomplete(reason),
        None => Inspection::plain(State::Active),
    }
}

pub(crate) fn write(path: &Path, binary: &Path, backup: bool) -> Result<WriteOutcome, String> {
    let mut doc = read(path)?.unwrap_or_default();
    if doc.get(CONTAINER).is_none() {
        doc[CONTAINER] = Item::Table(toml_edit::Table::new());
    }
    let servers = doc[CONTAINER]
        .as_table_like_mut()
        .ok_or_else(|| format!("{} {CONTAINER} must be a table", path.display()))?;
    if servers.get(SERVER_KEY).is_none() {
        servers.insert(SERVER_KEY, Item::Table(toml_edit::Table::new()));
    }
    let entry = servers
        .get_mut(SERVER_KEY)
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| {
            format!(
                "[{CONTAINER}.{SERVER_KEY}] in {} must be a table",
                path.display()
            )
        })?;
    // Field-wise, like the JSON side: only the keys topos owns are replaced.
    entry.insert("command", value(binary.display().to_string()));
    entry.insert("args", value(mcp_args()));
    atomic_write(path, &doc.to_string(), backup)
}

pub(crate) fn remove(path: &Path, dry_run: bool) -> Result<bool, String> {
    let Some(mut doc) = read(path)? else {
        return Ok(false);
    };
    if !owns_entry(&doc) {
        return Ok(false);
    }
    let emptied = {
        let Some(servers) = doc.get_mut(CONTAINER).and_then(Item::as_table_like_mut) else {
            return Ok(false);
        };
        servers.remove(SERVER_KEY);
        servers.is_empty()
    };
    if emptied {
        doc.remove(CONTAINER);
    }
    if !dry_run {
        atomic_write(path, &doc.to_string(), false)?;
    }
    Ok(true)
}

pub(crate) fn duplicate_keys(path: &Path, binary: &Path) -> Vec<String> {
    let Ok(Some(doc)) = read(path) else {
        return Vec::new();
    };
    let Some(servers) = doc.get(CONTAINER).and_then(Item::as_table_like) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter(|(key, _)| *key != SERVER_KEY)
        .filter(|(_, entry)| {
            entry
                .as_table_like()
                .and_then(|table| table.get("command").and_then(Item::as_str))
                .is_some_and(|command| points_at_topos(command, binary))
        })
        .map(|(key, _)| key.to_string())
        .collect()
}

fn read(path: &Path) -> Result<Option<DocumentMut>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    text.parse::<DocumentMut>()
        .map(Some)
        .map_err(|e| format!("parsing {}: {e}", path.display()))
}

fn entry_of(doc: &DocumentMut) -> Option<&dyn TableLike> {
    doc.get(CONTAINER)?
        .as_table_like()?
        .get(SERVER_KEY)?
        .as_table_like()
}

fn owns_entry(doc: &DocumentMut) -> bool {
    let Some(entry) = entry_of(doc) else {
        return false;
    };
    let command = entry
        .get("command")
        .and_then(Item::as_str)
        .unwrap_or_default();
    names_topos(command) && args_are_mcp(entry)
}

fn args_are_mcp(entry: &dyn TableLike) -> bool {
    entry
        .get("args")
        .and_then(Item::as_array)
        .map(|args| {
            args.iter()
                .filter_map(|arg| arg.as_str())
                .collect::<Vec<_>>()
                == MCP_ARGS
        })
        .unwrap_or(false)
}

fn mcp_args() -> Array {
    let mut args = Array::new();
    for arg in MCP_ARGS {
        args.push(arg);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::install::artifact::Artifact;
    use crate::commands::install::fsops::backup_path;
    use crate::commands::install::testing::tmp_dir;

    fn fake_binary(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("topos");
        fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn entry_round_trips_and_preserves_comments_and_unrelated_tables() {
        let dir = tmp_dir("round-trip");
        let binary = fake_binary(&dir);
        let path = dir.join("config.toml");
        fs::write(&path, "# my codex config\n[model]\nname = \"gpt\"\n").unwrap();

        let art = Artifact::McpToml;
        assert_eq!(art.inspect(&path, &binary).state, State::Absent);
        assert!(art.apply(&path, &binary).unwrap().is_some());
        assert_eq!(art.inspect(&path, &binary).state, State::Active);
        assert!(art.apply(&path, &binary).unwrap().is_none());

        let with_entry = fs::read_to_string(&path).unwrap();
        assert!(with_entry.contains("# my codex config"));
        assert!(with_entry.contains(&binary.display().to_string()));

        assert!(art.remove(&path, false).unwrap());
        assert_eq!(art.inspect(&path, &binary).state, State::Absent);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# my codex config"), "comment lost");
        assert!(text.contains("name = \"gpt\""));
        assert!(!text.contains(CONTAINER), "emptied table left behind");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn dry_run_removal_touches_nothing() {
        let dir = tmp_dir("dry-run");
        let binary = fake_binary(&dir);
        let path = dir.join("config.toml");
        Artifact::McpToml.apply(&path, &binary).unwrap();
        let before = fs::read_to_string(&path).unwrap();

        assert!(Artifact::McpToml.remove(&path, true).unwrap());

        assert_eq!(fs::read_to_string(&path).unwrap(), before);
        assert_eq!(
            Artifact::McpToml.inspect(&path, &binary).state,
            State::Active
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn drift_is_repaired_without_replacing_the_pristine_backup() {
        let dir = tmp_dir("drift");
        let binary = fake_binary(&dir);
        let path = dir.join("config.toml");
        fs::write(&path, "[model]\nname = \"gpt\"\n").unwrap();

        Artifact::McpToml.apply(&path, &binary).unwrap();
        let pristine = fs::read_to_string(backup_path(&path)).unwrap();
        assert!(!pristine.contains(CONTAINER));

        let stale = fs::read_to_string(&path)
            .unwrap()
            .replace(&binary.display().to_string(), "topos");
        fs::write(&path, stale).unwrap();
        assert_eq!(
            Artifact::McpToml.inspect(&path, &binary).state,
            State::Incomplete
        );

        Artifact::McpToml.apply(&path, &binary).unwrap();
        assert_eq!(
            Artifact::McpToml.inspect(&path, &binary).state,
            State::Active
        );
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), pristine);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_hand_made_entry_under_our_key_is_a_conflict_and_survives_uninstall() {
        let dir = tmp_dir("foreign");
        let binary = fake_binary(&dir);
        let path = dir.join("config.toml");
        let seed = "[mcp_servers.topos]\ncommand = \"uvx\"\nargs = [\"topos-mcp\"]\n";
        fs::write(&path, seed).unwrap();

        assert_eq!(
            Artifact::McpToml.inspect(&path, &binary).state,
            State::Conflict
        );
        assert!(!Artifact::McpToml.remove(&path, false).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), seed);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_sibling_server_survives_removal() {
        let dir = tmp_dir("sibling");
        let binary = fake_binary(&dir);
        let path = dir.join("config.toml");
        fs::write(&path, "[mcp_servers.other]\ncommand = \"foo\"\nargs = []\n").unwrap();

        Artifact::McpToml.apply(&path, &binary).unwrap();
        assert!(Artifact::McpToml.remove(&path, false).unwrap());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("mcp_servers.other"));
        assert!(!text.contains("mcp_servers.topos"));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn an_unparseable_config_is_a_conflict_and_is_left_untouched() {
        let dir = tmp_dir("unparseable");
        let binary = fake_binary(&dir);
        let path = dir.join("config.toml");
        fs::write(&path, "[not = toml").unwrap();

        assert_eq!(
            Artifact::McpToml.inspect(&path, &binary).state,
            State::Conflict
        );
        assert!(Artifact::McpToml.apply(&path, &binary).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[not = toml");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_foreign_key_pointing_at_topos_is_reported_as_a_duplicate() {
        let dir = tmp_dir("duplicates");
        let binary = fake_binary(&dir);
        let path = dir.join("config.toml");
        fs::write(
            &path,
            "[mcp_servers.topos-mcp]\ncommand = \"topos\"\nargs = [\"mcp\"]\n",
        )
        .unwrap();

        assert_eq!(
            Artifact::McpToml.duplicate_keys(&path, &binary),
            vec!["topos-mcp".to_string()]
        );
        fs::remove_dir_all(dir).ok();
    }
}
