//! `topos install` / `topos uninstall` / `topos status` — register the Topos
//! MCP server in agent harnesses, and take it back out without a trace.
//!
//! The command does exactly one job per harness: write the single MCP server
//! entry its config format expects, with an absolute `command` that actually
//! resolves. It never writes prose instruction blocks, `@import` lines or skill
//! files — those are shared with other tools or owned by a separate
//! distribution channel, so [`residue`] reports them and nothing here touches
//! them.
//!
//! Layout: [`harness`] is the 8-row table every command iterates; [`artifact`]
//! knows the three config shapes; [`binary`] decides which absolute path to
//! record; [`fsops`] does atomic writes and directory pruning; [`state`] tracks
//! what install created so uninstall removes exactly that much.

mod artifact;
mod binary;
mod configure;
mod fsops;
mod harness;
mod json_entry;
mod menu;
mod paths;
mod report;
mod residue;
mod state;
mod status;
mod toml_entry;
mod uninstall;

use std::io::IsTerminal;
use std::path::Path;

use clap::{Args, Subcommand};
use console::Term;

use artifact::State;
use harness::{ids, spec, HARNESSES};
use menu::{run_menu, MenuOption};
use paths::home_dir;

#[derive(Args)]
pub struct InstallArgs {
    #[command(subcommand)]
    command: Option<InstallCommand>,
    /// Harness ids to configure. Omit for interactive selection in a TTY, or
    /// pass --all.
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
    /// Harness ids to remove. Omit for interactive selection in a TTY, or pass
    /// --all.
    harnesses: Vec<String>,
    /// Target every supported harness.
    #[arg(long)]
    all: bool,
    /// Preview the removal without writing anything.
    #[arg(long)]
    dry_run: bool,
    /// Skip the confirmation prompt.
    #[arg(long, short = 'y')]
    yes: bool,
    /// Also delete the `.topos.backup` files left by earlier installs.
    #[arg(long)]
    purge_backups: bool,
}

/// Which of the three standard streams are terminals. Resolved once so the gate
/// below is a pure function of them, and every combination is unit-testable.
#[derive(Clone, Copy)]
pub(crate) struct Streams {
    pub(crate) stderr: bool,
    pub(crate) stdout: bool,
    pub(crate) stdin: bool,
}

impl Streams {
    fn detect() -> Self {
        Self {
            stderr: std::io::stderr().is_terminal(),
            stdout: std::io::stdout().is_terminal(),
            stdin: std::io::stdin().is_terminal(),
        }
    }
}

pub(crate) enum Interactivity {
    /// stderr is a tty, so prompt on it. Both [`menu`] and [`confirm`] read
    /// `Term::stderr()`; the gate and the read must stay the same stream, or a
    /// pty-allocated CI job prompts and then blocks forever.
    Interactive,
    /// No stream is a tty — a true non-interactive run, where uninstall applies.
    Headless,
    /// stderr is redirected but stdout or stdin is a tty: a human who typed
    /// `topos uninstall 2>log.txt`. Applying destructively here would mean no
    /// preview and no prompt, so uninstall reports and exits non-zero instead.
    Ambiguous,
}

pub(crate) fn interactivity(streams: Streams) -> Interactivity {
    match streams {
        Streams { stderr: true, .. } => Interactivity::Interactive,
        Streams {
            stdout: false,
            stdin: false,
            ..
        } => Interactivity::Headless,
        _ => Interactivity::Ambiguous,
    }
}

pub fn run_install(args: InstallArgs) -> Result<(), String> {
    let home = home_dir()?;
    if let Some(InstallCommand::Status(status_args)) = args.command {
        return status::run(&home, status_args.json);
    }
    let binary = binary::resolve_binary_path()?;
    let selected = match select_targets(&args, &home)? {
        Some(ids) => ids,
        None => return Ok(()),
    };
    if selected.is_empty() {
        println!("No integrations selected.");
        return Ok(());
    }
    configure::run(&home, &binary, &selected, args.dry_run)
}

fn select_targets(args: &InstallArgs, home: &Path) -> Result<Option<Vec<String>>, String> {
    if !args.harnesses.is_empty() || args.all {
        return validate_ids(&args.harnesses, args.all).map(Some);
    }
    if !matches!(interactivity(Streams::detect()), Interactivity::Interactive) {
        return Err(
            "non-interactive shells must pass explicit harness names or --all \
(see `topos install --help`)"
                .to_string(),
        );
    }
    interactive_select(home, Selection::Install)
}

pub fn run_uninstall(args: UninstallArgs) -> Result<(), String> {
    let home = home_dir()?;
    let mode = interactivity(Streams::detect());
    let selected = match uninstall_targets(&args, &home, &mode)? {
        Some(ids) => ids,
        None => return Ok(()),
    };
    if selected.is_empty() {
        println!("No integrations selected for removal.");
        return Ok(());
    }
    if args.dry_run {
        return uninstall::run(&home, &selected, true, false);
    }
    if args.yes || matches!(mode, Interactivity::Headless) {
        return uninstall::run(&home, &selected, false, args.purge_backups);
    }
    uninstall::run(&home, &selected, true, false)?;
    match mode {
        Interactivity::Interactive if confirm("Apply these changes?")? => {
            uninstall::run(&home, &selected, false, args.purge_backups)
        }
        Interactivity::Interactive => Ok(()),
        // stderr is redirected, so the preview above went somewhere the user may
        // never see and there is no stream left to prompt on.
        _ => Err(
            "stderr is not a terminal, so there is nowhere to confirm — \
re-run with --yes to apply, or --dry-run to preview"
                .to_string(),
        ),
    }
}

fn uninstall_targets(
    args: &UninstallArgs,
    home: &Path,
    mode: &Interactivity,
) -> Result<Option<Vec<String>>, String> {
    if !args.harnesses.is_empty() || args.all {
        return validate_ids(&args.harnesses, args.all).map(Some);
    }
    if matches!(mode, Interactivity::Interactive) {
        return interactive_select(home, Selection::Uninstall);
    }
    // Nothing to pick from and nowhere to ask: fall back to every harness that
    // still has something of ours in it.
    let binary = binary::resolve_binary_path()?;
    Ok(Some(
        HARNESSES
            .iter()
            .filter(|spec| {
                spec.artifact
                    .inspect(&(spec.config_path)(home), &binary)
                    .state
                    != State::Absent
            })
            .map(|spec| spec.id.to_string())
            .collect(),
    ))
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
    let binary = binary::resolve_binary_path()?;
    let options = HARNESSES
        .iter()
        .map(|spec| {
            let state = spec
                .artifact
                .inspect(&(spec.config_path)(home), &binary)
                .state;
            let (hint, hint_style, checked) = menu_hint(mode, state, || (spec.detect)(home));
            MenuOption {
                id: spec.id,
                name: spec.name,
                hint,
                hint_style,
                checked,
                // Blue radio only while the integration is already settled active.
                is_active: state == State::Active,
            }
        })
        .collect();
    run_menu(title, options)
}

/// Hint body, how to paint it, and whether the row starts checked.
fn menu_hint(
    mode: Selection,
    state: State,
    detected: impl Fn() -> bool,
) -> (String, menu::HintStyle, bool) {
    match (mode, state) {
        (_, State::Active) => ("active".to_string(), menu::HintStyle::Active, true),
        (_, State::Incomplete) => ("needs repair".to_string(), menu::HintStyle::Repair, true),
        // A conflict needs a human decision, so it is shown but never
        // pre-checked: install would refuse and uninstall would skip it.
        (_, State::Conflict) => (
            "conflict — inspect by hand".to_string(),
            menu::HintStyle::Plain,
            false,
        ),
        (Selection::Install, State::Absent) if detected() => {
            ("detected".to_string(), menu::HintStyle::Plain, true)
        }
        (Selection::Install, State::Absent) => {
            ("not configured".to_string(), menu::HintStyle::Plain, false)
        }
        (Selection::Uninstall, State::Absent) => {
            ("not configured".to_string(), menu::HintStyle::Plain, false)
        }
    }
}

fn confirm(prompt: &str) -> Result<bool, String> {
    use std::io::Write as _;
    eprint!("{prompt} [y/N] ");
    std::io::stderr().flush().map_err(|e| e.to_string())?;
    let key = Term::stderr().read_key().map_err(|e| e.to_string())?;
    eprintln!();
    Ok(matches!(key, console::Key::Char('y' | 'Y')))
}

pub fn run_status(args: StatusArgs) -> Result<(), String> {
    status::run(&home_dir()?, args.json)
}

fn validate_ids(requested: &[String], all: bool) -> Result<Vec<String>, String> {
    if all {
        return Ok(ids().iter().map(|id| (*id).to_string()).collect());
    }
    let mut unknown = Vec::new();
    let mut selected: Vec<String> = Vec::new();
    for id in requested {
        let lower = id.to_ascii_lowercase();
        match spec(&lower) {
            Some(_) if selected.contains(&lower) => {}
            Some(_) => selected.push(lower),
            None => unknown.push(id.clone()),
        }
    }
    if !unknown.is_empty() {
        return Err(format!(
            "unknown harness(es): {} (supported: {})",
            unknown.join(", "),
            ids().join(", ")
        ));
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn streams(stderr: bool, stdout: bool, stdin: bool) -> Streams {
        Streams {
            stderr,
            stdout,
            stdin,
        }
    }

    #[test]
    fn a_terminal_on_stderr_is_always_interactive() {
        for (stdout, stdin) in [(true, true), (false, false), (true, false)] {
            assert!(matches!(
                interactivity(streams(true, stdout, stdin)),
                Interactivity::Interactive
            ));
        }
    }

    #[test]
    fn only_a_fully_redirected_run_counts_as_headless() {
        assert!(matches!(
            interactivity(streams(false, false, false)),
            Interactivity::Headless
        ));
    }

    #[test]
    fn a_human_redirecting_only_stderr_is_ambiguous_not_headless() {
        // `topos uninstall 2>log.txt` from a terminal. Treating this as
        // headless would apply a destructive change with no preview and no
        // prompt, because the preview went to the redirected stream.
        for (stdout, stdin) in [(true, true), (true, false), (false, true)] {
            assert!(matches!(
                interactivity(streams(false, stdout, stdin)),
                Interactivity::Ambiguous
            ));
        }
    }

    #[test]
    fn unknown_harness_ids_are_rejected_with_the_supported_list() {
        let error = validate_ids(&["nope".to_string()], false).unwrap_err();
        assert!(error.contains("nope"));
        assert!(error.contains("claude"));
        assert!(error.contains("antigravity"));
    }

    #[test]
    fn ids_are_case_insensitive_and_deduplicated() {
        let selected = validate_ids(&["Claude".to_string(), "claude".to_string()], false).unwrap();
        assert_eq!(selected, vec!["claude".to_string()]);
    }

    #[test]
    fn all_selects_every_harness_in_table_order() {
        assert_eq!(validate_ids(&[], true).unwrap(), ids().to_vec());
    }

    #[test]
    fn conflicts_are_shown_but_never_preselected() {
        for mode in [Selection::Install, Selection::Uninstall] {
            let (hint, style, checked) = menu_hint(mode, State::Conflict, || true);
            assert!(!checked, "a conflict needs a human decision first");
            assert!(hint.contains("conflict"));
            assert_eq!(style, menu::HintStyle::Plain);
        }
    }

    #[test]
    fn install_preselects_detected_harnesses_but_uninstall_does_not() {
        let (hint, style, checked) = menu_hint(Selection::Install, State::Absent, || true);
        assert_eq!(hint, "detected");
        assert_eq!(style, menu::HintStyle::Plain);
        assert!(checked);
        let (_, _, checked) = menu_hint(Selection::Uninstall, State::Absent, || true);
        assert!(!checked, "nothing to remove from an unconfigured harness");
    }

    #[test]
    fn drifted_entries_are_preselected_for_repair() {
        let (hint, style, checked) = menu_hint(Selection::Install, State::Incomplete, || false);
        assert_eq!(hint, "needs repair");
        assert_eq!(style, menu::HintStyle::Repair);
        assert!(checked);
    }

    #[test]
    fn active_entries_use_active_hint_style() {
        let (hint, style, checked) = menu_hint(Selection::Install, State::Active, || false);
        assert_eq!(hint, "active");
        assert_eq!(style, menu::HintStyle::Active);
        assert!(checked);
    }
}
