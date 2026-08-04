//! `topos uninstall` — remove Topos-owned MCP registrations and leave no trace.
//!
//! "No trace" is the demanding half. Removing the entry is easy; what the draft
//! got wrong was everything after it — the emptied config files topos created,
//! the directories it created to hold them, its own backup files, and the state
//! file recording all of it. The order those come off matters and is fixed in
//! [`clean_up`].
//!
//! Nothing outside a Topos-owned MCP entry is ever removed. Prose instruction
//! blocks, `@import` lines and skill files are reported by
//! [`super::residue`] and left exactly where they are.

use std::path::{Path, PathBuf};

use super::artifact::State;
use super::fsops::{backup_path, prune_dirs, read_json_object};
use super::harness::{spec, HarnessSpec, HARNESSES};
use super::report;
use super::state;
use crate::commands::render::RenderOptions;

/// One line the confirm UI (or `--dry-run`) shows for a planned removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedAction {
    pub(crate) summary: String,
}

/// Build the uninstall plan without touching the filesystem or printing.
///
/// Interactive confirm uses this instead of a separate dry-run report so the
/// user sees "here is what will happen" and No/Yes in one block.
pub(crate) fn plan(home: &Path, selected: &[String], purge_backups: bool) -> Vec<PlannedAction> {
    let binary = super::binary::resolve_binary_path().unwrap_or_else(|_| PathBuf::from("topos"));
    let mut actions = Vec::new();
    for id in selected {
        let Some(harness) = spec(id) else {
            actions.push(PlannedAction {
                summary: format!("unknown harness: {id}"),
            });
            continue;
        };
        let path = (harness.config_path)(home);
        let inspection = harness.artifact.inspect(&path, &binary);
        let summary = match inspection.state {
            State::Absent => format!("{} — already clear", harness.name),
            State::Conflict => format!(
                "{} — left untouched ({})",
                harness.name,
                inspection.detail.unwrap_or_else(|| "conflict".to_string())
            ),
            _ => format!(
                "{} — remove MCP entry from {}",
                harness.name,
                display_path(home, &path)
            ),
        };
        actions.push(PlannedAction { summary });
        if purge_backups {
            let backup = backup_path(&path);
            if backup.is_file() {
                actions.push(PlannedAction {
                    summary: format!(
                        "{} — delete backup {}",
                        harness.name,
                        display_path(home, &backup)
                    ),
                });
            }
        }
    }
    actions
}

fn display_path(home: &Path, path: &Path) -> String {
    path.strip_prefix(home)
        .map(|rest| format!("~/{}", rest.display()))
        .unwrap_or_else(|_| path.display().to_string())
}

pub(crate) fn run(
    home: &Path,
    selected: &[String],
    dry_run: bool,
    purge_backups: bool,
) -> Result<(), String> {
    let opts = RenderOptions::stdout();
    report::header("Topos Harness Uninstall", dry_run, opts);

    let binary = super::binary::resolve_binary_path().unwrap_or_else(|_| PathBuf::from("topos"));
    let outcomes: Vec<bool> = selected
        .iter()
        .map(|id| remove_one(id, home, &binary, dry_run, opts))
        .collect();

    clean_up(home, selected, dry_run, purge_backups, opts);
    finish(outcomes.iter().all(|ok| *ok), dry_run, opts)
}

fn remove_one(id: &str, home: &Path, binary: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    let Some(harness) = spec(id) else {
        report::detail(&report::failed(opts), &format!("unknown harness: {id}"));
        return false;
    };
    report::harness_line(harness.name, opts);
    let success = remove_spec(harness, home, binary, dry_run, opts);
    println!("│");
    success
}

fn remove_spec(
    harness: &HarnessSpec,
    home: &Path,
    binary: &Path,
    dry_run: bool,
    opts: RenderOptions,
) -> bool {
    let path = (harness.config_path)(home);
    let inspection = harness.artifact.inspect(&path, binary);
    match inspection.state {
        State::Absent => {
            report::detail(&report::absent(opts), harness.absent_msg);
            true
        }
        // Not ours to remove — a hand-made entry under the `topos` key, or a
        // file topos cannot parse. Either way, deleting it would be guessing.
        State::Conflict => {
            report::detail(
                &report::conflict(opts),
                &inspection
                    .detail
                    .unwrap_or_else(|| format!("{} left untouched", path.display())),
            );
            true
        }
        _ if dry_run => {
            report::detail(
                &report::removed(opts),
                &format!(
                    "would remove the MCP server entry from {}",
                    display_path(home, &path)
                ),
            );
            true
        }
        _ => apply_removal(harness, home, &path, opts),
    }
}

fn apply_removal(harness: &HarnessSpec, home: &Path, path: &Path, opts: RenderOptions) -> bool {
    match harness.artifact.remove(path, false) {
        Ok(_) => {
            delete_if_emptied_and_ours(home, harness.id, path);
            report::detail(
                &report::removed(opts),
                &format!("removed the MCP server entry from {}", path.display()),
            );
            true
        }
        Err(message) => {
            report::detail(&report::failed(opts), &message);
            false
        }
    }
}

/// Delete a config file only when `topos install` created it AND removing our
/// entry left nothing else in it.
///
/// A pre-existing file that happens to end up empty is the user's, not ours, so
/// the ownership ledger — not emptiness alone — is what authorizes the delete.
fn delete_if_emptied_and_ours(home: &Path, id: &str, path: &Path) {
    if !state::was_created_by_install(home, id, path) {
        return;
    }
    if !is_empty_config(path) {
        return;
    }
    std::fs::remove_file(path).ok();
}

/// True when the file holds no meaningful content left. TOML is judged by
/// whitespace because a config topos created contains only its own table.
fn is_empty_config(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    match read_json_object(path) {
        Ok(map) => map.is_empty(),
        // Not JSON: fall back to "is there anything but whitespace".
        Err(_) => std::fs::read_to_string(path)
            .map(|text| text.trim().is_empty())
            .unwrap_or(false),
    }
}

/// Everything after the entries themselves, in the one order that works.
///
/// `install.json` lives inside a directory that is itself a prune candidate, so
/// the directory list has to be read before the state file is deleted, and the
/// state directory pruned after it.
fn clean_up(
    home: &Path,
    selected: &[String],
    dry_run: bool,
    purge_backups: bool,
    opts: RenderOptions,
) {
    if dry_run {
        return;
    }
    if purge_backups {
        purge(home, selected, opts);
    }
    for id in selected {
        state::clear_created_files(home, id).ok();
    }
    if !uninstalled_everything(home) {
        return;
    }
    let dirs = state::created_dirs(home);
    let pruned = prune_dirs(&dirs, false);
    state::remove_state_file(home).ok();
    prune_dirs(&[state::state_dir(home)], false);
    for dir in pruned {
        report::detail(
            &report::removed(opts),
            &format!("removed {}", dir.display()),
        );
    }
}

/// True when no harness still holds a Topos registration, so the shared state
/// file and the directories it records are safe to take down.
fn uninstalled_everything(home: &Path) -> bool {
    let binary = super::binary::resolve_binary_path().unwrap_or_else(|_| PathBuf::from("topos"));
    HARNESSES.iter().all(|harness| {
        matches!(
            harness
                .artifact
                .inspect(&(harness.config_path)(home), &binary)
                .state,
            State::Absent | State::Conflict
        )
    })
}

/// Delete the `.topos.backup` snapshots topos took for the harnesses being
/// uninstalled.
///
/// Scoped to `selected` rather than the whole table: a backup is the only copy
/// of a config as it stood before topos touched it, so
/// `topos uninstall codex --purge-backups` must not destroy the snapshot
/// belonging to a Claude Code install the user is keeping. The candidate paths
/// still come from the harness table rather than a hardcoded list, so a newly
/// added harness cannot be forgotten.
fn purge(home: &Path, selected: &[String], opts: RenderOptions) {
    for harness in HARNESSES
        .iter()
        .filter(|harness| selected.iter().any(|id| id == harness.id))
    {
        let backup = backup_path(&(harness.config_path)(home));
        if backup.is_file() && std::fs::remove_file(&backup).is_ok() {
            report::detail(
                &report::removed(opts),
                &format!("removed backup {}", backup.display()),
            );
        }
    }
}

fn finish(success: bool, dry_run: bool, opts: RenderOptions) -> Result<(), String> {
    let message = if dry_run {
        "Preview only — nothing changed."
    } else {
        "Done."
    };
    report::footer(message, opts);
    if success {
        Ok(())
    } else {
        Err("one or more harnesses could not be cleaned up".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::install::testing::tmp_dir;
    use std::fs;

    #[test]
    fn plan_describes_removal_without_dry_run_wording() {
        let home = tmp_dir("uninstall-plan");
        let codex = home.join(".codex");
        fs::create_dir_all(&codex).unwrap();
        fs::write(
            codex.join("config.toml"),
            "[mcp_servers.topos]\ncommand = \"/usr/bin/topos\"\nargs = [\"mcp\"]\n",
        )
        .unwrap();

        let actions = plan(&home, &["codex".into()], false);
        assert_eq!(actions.len(), 1);
        let summary = &actions[0].summary;
        assert!(summary.contains("Codex CLI"), "{summary}");
        assert!(summary.contains("remove MCP entry"), "{summary}");
        assert!(summary.contains("~/.codex/config.toml"), "{summary}");
        assert!(
            !summary.to_ascii_lowercase().contains("dry run"),
            "{summary}"
        );
    }

    #[test]
    fn plan_marks_absent_harnesses_clearly() {
        let home = tmp_dir("uninstall-plan-absent");
        let actions = plan(&home, &["codex".into()], false);
        assert_eq!(actions.len(), 1);
        assert!(
            actions[0].summary.contains("already clear"),
            "{}",
            actions[0].summary
        );
    }
}
