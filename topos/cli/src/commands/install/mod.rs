//! `topos install` / `topos uninstall` — configure or remove Topos support
//! in agent harnesses (Claude Code, Claude Desktop, Codex CLI, Gemini CLI,
//! GitHub Copilot CLI, Cursor & VS Code, Google Antigravity).
//!
//! `integrations.rs` holds the harness table every command here loops over,
//! plus the schema notes for each harness's config format; `edits.rs` has the
//! per-format read-modify-write primitives, and `ownership.rs` tracks which
//! files install created so uninstall only deletes those.

mod configure;
mod edits;
mod integrations;
mod menu;
mod ownership;
mod status;
#[cfg(test)]
mod testing;
mod uninstall;

use std::path::Path;

use clap::{Args, Subcommand};
use console::Term;

use integrations::{harness, home_dir, supported_ids, State, HARNESSES};
use menu::{run_menu, MenuOption};

#[derive(Args)]
pub struct InstallArgs {
    #[command(subcommand)]
    command: Option<InstallCommand>,
    /// Harness ids to configure (claude, claude-desktop, codex, gemini,
    /// copilot, skills, antigravity). Omit for interactive selection in a
    /// TTY, or pass --all.
    harnesses: Vec<String>,
    /// Configure every harness detected on this machine.
    #[arg(long, conflicts_with = "harnesses")]
    all: bool,
    /// Preview changes without writing anything.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Subcommand)]
enum InstallCommand {
    /// Show which harnesses are configured for Topos.
    Status(StatusArgs),
}

#[derive(Args)]
pub struct StatusArgs {
    /// Emit machine-readable JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct UninstallArgs {
    /// Harness ids to remove. Omit for interactive selection in a TTY, or
    /// pass --all.
    harnesses: Vec<String>,
    /// Target every harness that currently has Topos configured.
    #[arg(long, conflicts_with = "harnesses")]
    all: bool,
    /// Actually remove Topos-owned entries. Without this, uninstall only
    /// previews what would change.
    #[arg(long)]
    apply: bool,
    /// Also delete the `.topos.backup` files left by earlier installs.
    #[arg(long)]
    purge_backups: bool,
}

#[derive(Clone, Copy)]
enum Selection {
    Install,
    Uninstall,
}

impl Selection {
    fn title(self) -> &'static str {
        match self {
            Selection::Install => "Which agent integrations do you want to configure?",
            Selection::Uninstall => "Which agent integrations do you want to uninstall?",
        }
    }

    /// What `--all` means. Install targets the harnesses actually present on
    /// this machine, so it does not create `~/.copilot` and friends on a box
    /// that never had them; uninstall targets whatever Topos has configured.
    fn matches_all(self, state: State, detected: bool) -> bool {
        match self {
            Selection::Install => detected || state != State::Absent,
            Selection::Uninstall => state != State::Absent,
        }
    }
}

pub fn run_install(args: InstallArgs) -> Result<(), String> {
    let home = home_dir()?;
    if let Some(InstallCommand::Status(status_args)) = args.command {
        return status::run(&home, status_args.json);
    }
    let Some(selected) = resolve_targets(
        &home,
        &args.harnesses,
        args.all,
        Selection::Install,
        "topos install",
    )?
    else {
        return Ok(());
    };
    if selected.is_empty() {
        println!("No integrations selected.");
        return Ok(());
    }
    configure::run(&home, &selected, args.dry_run)
}

pub fn run_uninstall(args: UninstallArgs) -> Result<(), String> {
    let home = home_dir()?;
    let Some(selected) = resolve_targets(
        &home,
        &args.harnesses,
        args.all,
        Selection::Uninstall,
        "topos uninstall",
    )?
    else {
        return Ok(());
    };
    if selected.is_empty() {
        println!("No integrations selected for removal.");
        return Ok(());
    }

    let mut apply = args.apply;
    if !apply {
        uninstall::run(&home, &selected, true, false)?;
        if Term::stderr().is_term() {
            apply = confirm("Apply these changes?")?;
        }
    }
    if apply {
        uninstall::run(&home, &selected, false, args.purge_backups)
    } else {
        Ok(())
    }
}

/// Explicit names win; `--all` expands per [`Selection::matches_all`]; a bare
/// invocation opens the menu in a TTY. `Ok(None)` means the user cancelled.
///
/// A non-interactive shell that names nothing is an error for both commands —
/// inferring the targets of an `--apply`'d uninstall from whatever happens to
/// be configured is exactly the mistake that flag exists to prevent.
fn resolve_targets(
    home: &Path,
    harnesses: &[String],
    all: bool,
    mode: Selection,
    command: &str,
) -> Result<Option<Vec<String>>, String> {
    if !harnesses.is_empty() {
        return validate_ids(harnesses).map(Some);
    }
    if all {
        return Ok(Some(
            HARNESSES
                .iter()
                .filter(|entry| mode.matches_all(entry.state(home), entry.is_detected(home)))
                .map(|entry| entry.id.to_string())
                .collect(),
        ));
    }
    if Term::stderr().is_term() {
        return interactive_select(home, mode);
    }
    Err(format!(
        "non-interactive shells must pass explicit harness names or --all \
(see `{command} --help`)"
    ))
}

fn interactive_select(home: &Path, mode: Selection) -> Result<Option<Vec<String>>, String> {
    let options = HARNESSES
        .iter()
        .map(|entry| {
            let (hint, checked) = match (mode, entry.state(home)) {
                (_, State::Active) => ("active".to_string(), true),
                (_, State::Stale) => ("stale — needs repair".to_string(), true),
                (Selection::Install, State::Absent) => {
                    let detected = entry.is_detected(home);
                    let hint = if detected {
                        "detected"
                    } else {
                        "not installed"
                    };
                    (hint.to_string(), detected)
                }
                (Selection::Uninstall, State::Absent) => ("not configured".to_string(), false),
            };
            MenuOption {
                id: entry.id,
                name: entry.name,
                hint,
                checked,
            }
        })
        .collect();
    run_menu(mode.title(), options)
}

fn confirm(prompt: &str) -> Result<bool, String> {
    use std::io::Write as _;
    eprint!("{prompt} [y/N] ");
    std::io::stderr().flush().map_err(|e| e.to_string())?;
    let key = Term::stderr().read_key().map_err(|e| e.to_string())?;
    eprintln!();
    Ok(matches!(key, console::Key::Char('y' | 'Y')))
}

fn validate_ids(ids: &[String]) -> Result<Vec<String>, String> {
    let mut unknown = Vec::new();
    let mut selected = Vec::new();
    for id in ids {
        let lower = id.to_ascii_lowercase();
        match harness(&lower) {
            Some(_) if selected.contains(&lower) => {}
            Some(_) => selected.push(lower),
            None => unknown.push(id.clone()),
        }
    }
    if !unknown.is_empty() {
        return Err(format!(
            "unknown harness(es): {} (supported: {})",
            unknown.join(", "),
            supported_ids().join(", ")
        ));
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_ids_are_rejected_with_the_supported_list() {
        let error = validate_ids(&["claude".to_string(), "emacs".to_string()]).unwrap_err();
        assert!(error.contains("emacs"), "got: {error}");
        assert!(error.contains("claude-desktop"), "got: {error}");
    }

    #[test]
    fn ids_are_lowercased_and_deduplicated() {
        let ids = validate_ids(&["Claude".to_string(), "claude".to_string()]).unwrap();
        assert_eq!(ids, vec!["claude".to_string()]);
    }

    /// The mistake `--apply` exists to prevent: a non-interactive uninstall
    /// that names nothing must not infer its targets. stderr is not a TTY
    /// under `cargo test`, so this exercises the non-interactive branch.
    #[test]
    fn a_non_interactive_run_without_targets_is_an_error() {
        let home = Path::new("/home/example");
        for mode in [Selection::Install, Selection::Uninstall] {
            assert!(resolve_targets(home, &[], false, mode, "topos install").is_err());
        }
    }

    /// `--all` must not create config directories for harnesses that were
    /// never installed here.
    #[test]
    fn install_all_skips_undetected_harnesses() {
        let home = Path::new("/nonexistent-home-for-tests");
        let selected = resolve_targets(home, &[], true, Selection::Install, "topos install")
            .unwrap()
            .unwrap();
        assert!(selected.is_empty(), "got: {selected:?}");
    }
}
