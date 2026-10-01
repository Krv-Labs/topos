//! `topos update` — check for a newer release and offer to install it.
//!
//! Answers the question `install.sh` can only answer by printing: *which
//! channel is this binary from, and what command upgrades it?* The upgrade
//! itself is delegated to that channel's own mechanism — `install.sh` for a
//! binary install, `brew upgrade` for a Homebrew one — rather than downloading
//! over a package manager's files, which the next `brew upgrade` would
//! silently revert.
//!
//! Nothing happens without a confirmation. `topos update` never runs a package
//! manager unasked; it reports what it found, shows the exact command, and
//! waits.
//!
//! Non-interactive runs **report and exit 0 without touching anything**. An
//! agent or a CI job has nobody to answer the prompt, and downloading a
//! binary nobody agreed to is not a safe default.

mod report;

use std::path::Path;

use clap::Args;
use console::Style as ConsoleStyle;
use topos_mcp::paths as mcp_paths;

use topos_mcp::update::{self, channel::Action, Install};

use crate::commands::install::binary;
use crate::commands::interaction::{self, Interaction, PromptEnv, Streams};
use crate::commands::menu;
use crate::commands::render::{self, RenderOptions};

#[derive(Args)]
pub struct UpdateArgs {
    /// Report what is installed and what is published, without offering to
    /// change anything. Implied when no terminal is available.
    #[arg(long)]
    check: bool,
    /// Apply the upgrade without prompting. Still only ever runs a channel's
    /// own upgrade command, never a download.
    #[arg(long, short = 'y')]
    yes: bool,
    /// Emit the survey as JSON instead of a card.
    #[arg(long)]
    json: bool,
}

/// Shown by `topos update --help`, because the channel rules are the part a
/// person needs before trusting the command with their machine.
pub const LONG_HELP: &str = "\
Channels:
  binary install   ~/.local/bin, installed by install.sh. Upgraded by re-running
                   install.sh, which verifies the release checksum before
                   replacing the binary.
  homebrew         Installed by `brew install krv-labs/tap/topos`. Upgraded by
                   `brew upgrade topos` — topos runs it for you, and does not
                   download over Homebrew's cellar.
  cargo            ~/.cargo/bin. Run `cargo install topos --force` yourself.
  source checkout  A `cargo build --release` tree. Run `git pull && cargo
                   build --release -p topos` yourself; topos does not know
                   which working tree you mean.

Topos never removes an install. When more than one topos is installed, $PATH
order decides which one runs — `topos update` lists them so the one you are
upgrading is the one you are calling.

The check is throttled to once every 24 hours. Set TOPOS_NO_UPDATE_NOTICES=1 to
silence the passive notices entirely.";

/// `topos update` — report, then offer.
pub fn run(args: UpdateArgs) -> Result<(), String> {
    let home = mcp_paths::home_dir()?;
    // Ask the release server, then cache the answer for the passive notice.
    // A repeat run within the throttle costs no request.
    let survey = update::survey(&home);
    let _ = topos_mcp::update::notice::save(&home, &notice_record(&home, &survey));

    if args.json {
        return print_json(&survey);
    }

    let interaction =
        interaction::resolve(args.yes, false, &PromptEnv::from_env(), &Streams::detect());
    let opts = RenderOptions::stdout();

    // Always print the survey: a person ran this and is owed an answer even
    // when nothing can be offered.
    report_survey(&survey, opts);

    // An update is never applied without being asked for. `--yes` is that
    // explicit ask; a terminal is an implicit one; anything else just reports.
    let may_apply = args.yes || matches!(interaction, Interaction::Prompt);
    if survey.outdated().is_empty() {
        return Ok(());
    }
    if args.check {
        return Ok(());
    }
    if !may_apply {
        print_manual(&survey, opts);
        return Ok(());
    }
    // `$PATH` order decides which binary a bare `topos` runs, which is the one
    // worth marking and the one an upgrade has to reach to change anything.
    let active = survey
        .installs
        .first()
        .map(|install| install.path.as_path());
    confirm_and_apply(&survey, active, opts)
}

/// The card, before any prompt.
fn report_survey(survey: &update::Survey, opts: RenderOptions) {
    let latest = survey.latest.clone();
    let mut lines = vec![render::paint(
        format!(
            "┌  {}",
            report::title(survey.update_available() == Some(true))
        ),
        ConsoleStyle::new().bold(),
        opts,
    )];
    lines.push(render::guide('│', opts));
    lines.push(render::guide_line(
        format!(
            "{} is published for {}.",
            latest.as_deref().unwrap_or("no release"),
            update::release::platform()
        ),
        ConsoleStyle::new().dim(),
        opts,
    ));
    lines.push(render::guide('│', opts));

    // Report the install that a bare `topos` would actually run — the first on
    // `$PATH`, which is `installs[0]` by construction — falling back to the
    // running binary only when `$PATH` resolves nothing. Leading with the
    // running binary instead would name a source checkout while the user types
    // `topos` and gets the Homebrew one.
    let primary = survey.installs.first();
    if let Some(install) = primary {
        lines.extend(report::metadata(
            &survey.current,
            latest.as_deref(),
            &format!("{} · {}", install.channel.label(), install.path.display()),
            install.channel.upgrade_command(),
            opts,
        ));
    }
    let primary_path = primary.map(|install| install.path.as_path());
    if survey.shadowed() {
        lines.push(render::guide('│', opts));
        // The mark is `$PATH`-order "this is the one that runs", which is what
        // makes an upgrade that looks ineffective explicable. It is not
        // necessarily the process doing the reporting.
        lines.extend(report::install_rows(&survey.installs, primary_path, opts));
        lines.push(render::guide_line(
            "PATH order decides which runs",
            ConsoleStyle::new().dim(),
            opts,
        ));
    }
    lines.push(render::guide('│', opts));
    render::print_lines(lines);
}

/// The cache record for what this survey learned.
///
/// Written on every explicit run so a `topos update --check` a user ran by
/// hand is also what the next passive notice reports. `notified_at` is
/// deliberately carried over from the previous record: this run reported
/// explicitly, so it has not *notified* anyone, and clobbering the field would
/// either suppress or duplicate the next notice.
fn notice_record(
    home: &std::path::Path,
    survey: &update::Survey,
) -> topos_mcp::update::notice::Record {
    let previous = topos_mcp::update::notice::load(home);
    topos_mcp::update::notice::Record {
        checked_at: topos_mcp::update::notice::now(),
        notified_at: previous.notified_at,
        latest: survey.latest.clone(),
        current: Some(survey.current.clone()),
        installs: survey
            .installs
            .iter()
            .map(|install| topos_mcp::update::notice::SeenInstall {
                path: install.path.display().to_string(),
                version: install.version.clone(),
                channel: install.channel.label().to_string(),
            })
            .collect(),
    }
}

/// Non-interactive: name the command instead of waiting for an answer.
fn print_manual(survey: &update::Survey, opts: RenderOptions) {
    let Some(install) = survey
        .installs
        .iter()
        .find(|install| install.channel.action() != Action::None)
        .or_else(|| survey.installs.first())
    else {
        render::print_lines([
            render::guide_line(
                "no topos binary was found on PATH",
                ConsoleStyle::new().dim(),
                opts,
            ),
            render::guide('└', opts),
        ]);
        return;
    };
    render::print_lines([
        render::guide_line(
            format!("{} is not a terminal, so run:", install.channel.label()),
            ConsoleStyle::new().dim(),
            opts,
        ),
        format!(
            "│    {}",
            render::paint(
                install.channel.upgrade_command(),
                ConsoleStyle::new().yellow().bold(),
                opts
            )
        ),
        render::guide_line(
            "or re-run with --yes to apply it here",
            ConsoleStyle::new().dim(),
            opts,
        ),
        render::guide('└', opts),
    ]);
}

/// Offer the upgrade, then run it.
///
/// One install goes through the single-select; several go through the
/// checkbox `topos install` uses, because "which of these" is plural. Either
/// way the exact commands are shown in the confirm plan before anything runs.
fn confirm_and_apply(
    survey: &update::Survey,
    active: Option<&Path>,
    opts: RenderOptions,
) -> Result<(), String> {
    let outdated: Vec<Install> = survey.outdated().into_iter().cloned().collect();
    let latest = survey.latest.clone();
    let mut lines = Vec::new();

    let chosen: Vec<Install> = if outdated.len() <= 1 {
        // One install: a single-select, matching the Kimi-style card — the
        // question is genuinely binary.
        lines.push(render::guide_line(
            latest
                .as_deref()
                .map(|latest| format!("{latest} is ready to install"))
                .unwrap_or_else(|| "the installer will fetch the newest release".into()),
            ConsoleStyle::new().dim(),
            opts,
        ));
        lines.push(render::guide('│', opts));
        render::print_lines(lines);
        let step = report::action_step(latest.as_deref(), &survey.current);
        let header = vec![render::guide('│', opts)];
        match menu::run_select(&header, &step)? {
            Some(0) => outdated,
            _ => return Ok(()),
        }
    } else {
        // Filter to what can actually be acted on; a channel topos cannot
        // upgrade is a manual step, not a checkbox.
        let actionable: Vec<Install> = outdated
            .iter()
            .filter(|install| install.channel.action() != Action::None)
            .cloned()
            .collect();
        let unhandled: Vec<&Install> = outdated
            .iter()
            .filter(|install| install.channel.action() == Action::None)
            .collect();
        if actionable.is_empty() {
            print_channels(&unhandled, opts);
            return Ok(());
        }
        if !unhandled.is_empty() {
            // Named before the prompt rather than after it, so the choice is
            // made with full knowledge of what the checkbox cannot cover.
            print_channels(&unhandled, opts);
        }
        let target = latest.clone().unwrap_or_else(|| survey.current.clone());
        let menu_options = report::install_menu(&actionable, active, &target);
        match menu::run_menu("Topos update available", menu_options)? {
            Some(ids) if !ids.is_empty() => actionable
                .into_iter()
                .filter(|install| {
                    ids.iter()
                        .any(|id| *id == install.path.display().to_string())
                })
                .collect(),
            _ => return Ok(()),
        }
    };

    if chosen.is_empty() {
        return Ok(());
    }
    let plan: Vec<String> = chosen
        .iter()
        .map(|install| install.channel.upgrade_command().to_string())
        .collect();
    if !menu::run_confirm("Apply this update?", &plan)? {
        return Ok(());
    }
    for install in &chosen {
        update::apply(install.channel)?;
    }
    announce(&chosen, opts);
    Ok(())
}

/// Channels topos will not touch, printed with their commands.
fn print_channels(installs: &[&Install], opts: RenderOptions) {
    for install in installs {
        render::print_lines([
            render::guide_line(
                format!(
                    "{} at {} cannot be upgraded by topos",
                    install.channel.label(),
                    install.path.display()
                ),
                ConsoleStyle::new().dim(),
                opts,
            ),
            format!(
                "│    {}",
                render::paint(
                    install.channel.upgrade_command(),
                    ConsoleStyle::new().yellow().bold(),
                    opts
                )
            ),
            render::guide('│', opts),
        ]);
    }
}

/// What changed, and what to do if the binary moved.
fn announce(_chosen: &[Install], opts: RenderOptions) {
    // A Homebrew upgrade swaps the cellar symlink target, and `install.sh`
    // replaces the binary in place. Neither *moves* the path a harness entry
    // records, so drift is checked rather than assumed: `binary::drift` is
    // the same rule `topos install` uses to decide an entry is stale.
    let recorded = binary::resolve_binary_path().ok();
    let mut lines = vec![render::paint(
        "◇  Topos updated",
        ConsoleStyle::new().bold(),
        opts,
    )];
    if let Some(path) = recorded {
        if let Some(reason) = binary::drift(&path.display().to_string(), &path) {
            lines.push(render::guide_line(
                format!("{reason} — run `topos install` to refresh harness entries"),
                ConsoleStyle::new().color256(208),
                opts,
            ));
        }
    }
    lines.push(render::guide('└', opts));
    render::print_lines(lines);
}

fn print_json(survey: &update::Survey) -> Result<(), String> {
    let installs: Vec<_> = survey
        .installs
        .iter()
        .map(|install| {
            serde_json::json!({
                "path": install.path.display().to_string(),
                "version": install.version,
                "channel": install.channel.label(),
                "updatable": install.channel.action() != Action::None,
                "command": install.channel.upgrade_command(),
            })
        })
        .collect();
    let body = serde_json::json!({
        "current": survey.current,
        "latest": survey.latest,
        "updateAvailable": survey.update_available(),
        "platform": update::release::platform(),
        "installs": installs,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&body)
            .map_err(|e| format!("cannot serialize the update survey: {e}"))?
    );
    Ok(())
}
