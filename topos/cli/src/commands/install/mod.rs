//! `topos install` / `topos uninstall` — configure or remove Topos support
//! in agent harnesses (Claude Code, Claude Desktop, Codex CLI, Gemini CLI,
//! GitHub Copilot CLI, Cursor & VS Code, Google Antigravity).
//!
//! Structure follows the harness-installer pattern in sgathrid/brian
//! (`wikicli/lifecycle/`): a small state module (`integrations`), an
//! install/uninstall pass per harness, a status report, and an interactive
//! TTY menu for picking targets. See `integrations.rs` for the schema
//! notes and how Topos's mechanism (MCP registration + a skill file)
//! differs from brian's session-start hooks.

mod configure;
mod integrations;
mod menu;
mod status;
mod uninstall;

use std::path::Path;

use clap::{Args, Subcommand};
use console::Term;

use integrations::{detect_dir, harness_name, home_dir, integration_state, State, SUPPORTED};
use menu::{run_menu, MenuOption};

#[derive(Args)]
pub struct InstallArgs {
    #[command(subcommand)]
    command: Option<InstallCommand>,
    /// Harness ids to configure (claude, claude-desktop, codex, gemini,
    /// copilot, skills, antigravity). Omit for interactive selection in a
    /// TTY, or pass --all.
    harnesses: Vec<String>,
    /// Configure every supported harness.
    #[arg(long)]
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
    /// Target every supported harness.
    #[arg(long)]
    all: bool,
    /// Actually remove Topos-owned entries. Without this, uninstall only
    /// previews what would change.
    #[arg(long)]
    apply: bool,
    /// Also delete the `.topos.backup` files left by earlier installs.
    #[arg(long)]
    purge_backups: bool,
}

pub fn run_install(args: InstallArgs) -> Result<(), String> {
    let home = home_dir()?;
    if let Some(InstallCommand::Status(status_args)) = args.command {
        return status::run(&home, status_args.json);
    }
    let explicit = !args.harnesses.is_empty() || args.all;
    let selected = if explicit {
        validate_ids(&args.harnesses, args.all)?
    } else if Term::stderr().is_term() {
        match interactive_select(&home, Selection::Install)? {
            Some(ids) => ids,
            None => return Ok(()),
        }
    } else {
        return Err(
            "non-interactive shells must pass explicit harness names or --all \
(see `topos install --help`)"
                .to_string(),
        );
    };
    if selected.is_empty() {
        println!("No integrations selected.");
        return Ok(());
    }
    configure::run(&home, &selected, args.dry_run)
}

pub fn run_uninstall(args: UninstallArgs) -> Result<(), String> {
    let home = home_dir()?;
    let interactive_tty = Term::stderr().is_term();
    let explicit = !args.harnesses.is_empty() || args.all;
    let selected = if explicit {
        validate_ids(&args.harnesses, args.all)?
    } else if interactive_tty {
        match interactive_select(&home, Selection::Uninstall)? {
            Some(ids) => ids,
            None => return Ok(()),
        }
    } else {
        SUPPORTED
            .iter()
            .copied()
            .map(str::to_string)
            .filter(|id| integration_state(id, &home) != State::Absent)
            .collect()
    };
    if selected.is_empty() {
        println!("No integrations selected for removal.");
        return Ok(());
    }

    let mut apply = args.apply;
    if !apply {
        uninstall::run(&home, &selected, true, false)?;
        if interactive_tty {
            apply = confirm("Apply these changes?")?;
        }
    }
    if apply {
        uninstall::run(&home, &selected, false, args.purge_backups)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Selection {
    Install,
    Uninstall,
}

fn interactive_select(home: &Path, mode: Selection) -> Result<Option<Vec<String>>, String> {
    let title = match mode {
        Selection::Install => "Which agent integrations do you want to configure?",
        Selection::Uninstall => "Which agent integrations do you want to uninstall?",
    };
    let options = SUPPORTED
        .iter()
        .copied()
        .map(|id| {
            let state = integration_state(id, home);
            let (hint, checked) = match (mode, state) {
                (_, State::Active) => ("active".to_string(), true),
                (_, State::Stale) => ("stale — needs repair".to_string(), true),
                (Selection::Install, State::Absent) => {
                    let detected = detect_dir(id, home).exists();
                    let hint = if detected {
                        "detected"
                    } else {
                        "not configured"
                    };
                    (hint.to_string(), detected)
                }
                (Selection::Uninstall, State::Absent) => ("not configured".to_string(), false),
            };
            MenuOption {
                id,
                name: harness_name(id),
                hint,
                checked,
            }
        })
        .collect();
    run_menu(title, options)
}

fn confirm(prompt: &str) -> Result<bool, String> {
    use std::io::Write as _;
    eprint!("{prompt} [y/N] ");
    std::io::stderr().flush().map_err(|e| e.to_string())?;
    let key = Term::stderr().read_key().map_err(|e| e.to_string())?;
    eprintln!();
    Ok(matches!(key, console::Key::Char('y' | 'Y')))
}

fn validate_ids(ids: &[String], all: bool) -> Result<Vec<String>, String> {
    if all {
        return Ok(SUPPORTED.iter().map(|s| s.to_string()).collect());
    }
    let mut unknown = Vec::new();
    let mut selected = Vec::new();
    for id in ids {
        let lower = id.to_ascii_lowercase();
        if SUPPORTED.contains(&lower.as_str()) {
            if !selected.contains(&lower) {
                selected.push(lower);
            }
        } else {
            unknown.push(id.clone());
        }
    }
    if !unknown.is_empty() {
        return Err(format!(
            "unknown harness(es): {} (supported: {})",
            unknown.join(", "),
            SUPPORTED.join(", ")
        ));
    }
    Ok(selected)
}
