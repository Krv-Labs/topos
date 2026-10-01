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

mod download;
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

    let streams = Streams::detect();
    let interaction = interaction::resolve(args.yes, false, &PromptEnv::from_env(), &streams);
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
    confirm_and_apply(&survey, active, opts, streams.stderr)
}

/// The card, before any prompt.
///
/// Two shapes. Up to date gets a short, closed report and nothing else — there
/// is no question to ask, so there must be no prompt. An available update gets
/// the metadata block, then the existing install table when more than one
/// binary is present.
fn report_survey(survey: &update::Survey, opts: RenderOptions) {
    let available = survey.update_available() == Some(true);
    let latest = survey.latest.clone();
    let mut lines = Vec::new();

    // Report the install that a bare `topos` would actually run — the first on
    // `$PATH`, which is `installs[0]` by construction. Leading with the
    // running binary instead would name a source checkout while the user types
    // `topos` and gets the Homebrew one.
    let primary = survey.installs.first();
    let primary_path = primary.map(|install| install.path.as_path());

    // `◇` headline, then the rail opens — the shape `config.rs:70` uses for a
    // finished, non-interactive card. The banner itself carries no rail.
    lines.push(render::paint(
        format!(
            "{}  {}",
            render::guide('◇', opts),
            report::headline(available, &survey.current, latest.as_deref(),)
        ),
        ConsoleStyle::new().bold(),
        opts,
    ));
    lines.push(render::guide('│', opts));

    if let Some(install) = primary {
        lines.extend(report::metadata(
            &survey.current,
            available.then_some(latest.as_deref()).flatten(),
            &format!("{} · {}", install.channel.label(), install.path.display()),
            available.then_some(install.channel.upgrade_command()),
            opts,
        ));
    }
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
    lines.push(render::guide('└', opts));
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
    draw: bool,
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
        .map(|install| describe(install, survey))
        .collect();
    if !menu::run_confirm("Apply this update?", &plan)? {
        return Ok(());
    }
    let chrome = download::Chrome::interactive(opts, draw);
    for install in &chosen {
        match install.channel.action() {
            Action::Download => {
                // Replace the binary this install *is*, not the running one: a
                // shadowed Homebrew binary and the one you typed are different
                // files, and upgrading the wrong one is how an update appears
                // to do nothing.
                let _ = download::install(
                    &install.path,
                    survey.latest.as_deref().unwrap_or(&survey.current),
                    &update::release::platform(),
                    chrome,
                )?;
            }
            other => update::apply_for(other, install.channel)?,
        }
    }
    announce(&chosen, opts);
    Ok(())
}

/// One plan line: what runs, and against what.
fn describe(install: &Install, survey: &update::Survey) -> String {
    match install.channel.action() {
        Action::Download => format!(
            "download and replace {} → {}",
            install.path.display(),
            survey.latest.as_deref().unwrap_or(&survey.current)
        ),
        _ => install.channel.upgrade_command().to_string(),
    }
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
fn announce(chosen: &[Install], opts: RenderOptions) {
    // Neither path moves: an in-place replace keeps the file where it was, and
    // `brew upgrade` swaps the cellar symlink's target rather than the `$PATH`
    // entry that points at it. So the "re-run `topos install`" advice is
    // checked with `binary::drift` — the same rule `topos install` uses to
    // decide an entry is stale — rather than printed whenever anything ran.
    let recorded = binary::resolve_binary_path().ok();
    let drifted = recorded
        .as_deref()
        .and_then(|path| binary::drift(&path.display().to_string(), path));

    let mut lines = vec![render::paint(
        format!("{}  Topos updated", render::guide('◇', opts)),
        ConsoleStyle::new().bold(),
        opts,
    )];
    match drifted {
        Some(reason) => lines.push(render::guide_line(
            format!("{reason} — run `topos install` so harness entries follow it"),
            ConsoleStyle::new().color256(208),
            opts,
        )),
        None => lines.push(render::guide_line(
            "the binary stayed where it was, so harness entries still resolve",
            ConsoleStyle::new().dim(),
            opts,
        )),
    }
    let _ = chosen;
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
