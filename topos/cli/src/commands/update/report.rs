//! The update card, as pure `Vec<String>`.
//!
//! Split from the prompt loop so every line can be asserted with
//! `styled: false` — the same reason `menu.rs` keeps `render_select`
//! separate. Nothing here reads a terminal, spawns a process, or touches the
//! filesystem.
//!
//! Grammar follows `menu.rs` exactly, so a `topos update` card is
//! indistinguishable from a `topos install` one: `┌  title`, `│  key`, blank
//! `│` rails, a `│ ` choice row per option, bare `└`. The `Current` /
//! `Target` / `Source` / `Command` block is the aligned key-value idiom from
//! `config.rs:102-153` — one padded label column so the values line up.

use std::path::Path;

use console::Style;

use topos_mcp::update::Install;

use crate::commands::menu::{SelectOption, SelectStep, StepLayout};
use crate::commands::render::{paint, RenderOptions};

/// Width of the metadata label column. `Target` / `Current` are the longest
/// keys that fit in the same shape as `config.rs`'s 12-character column.
const LABEL_WIDTH: usize = 8;

/// Label for the upgrade channel, dimmed beside the binary's channel name.
pub(crate) fn title(survey_latest: bool) -> String {
    if survey_latest {
        "Topos update available".to_string()
    } else {
        "Topos is up to date".to_string()
    }
}

/// The `Current` / `Target` / `Source` / `Command` rows.
///
/// `command` is the thing the user would otherwise have to go read the docs to
/// find, and it is long — so it wraps with a hanging indent under its own
/// label rather than being truncated. A truncated `curl` command is not a
/// command.
pub(crate) fn metadata(
    current: &str,
    latest: Option<&str>,
    source: &str,
    command: &str,
    opts: RenderOptions,
) -> Vec<String> {
    let rail = crate::commands::render::guide('│', opts);
    let mut lines = vec![guide_line(
        &rail,
        &format!("{:<width$}  {}", "Current", current, width = LABEL_WIDTH),
        opts,
    )];
    if let Some(latest) = latest {
        lines.push(guide_line(
            &rail,
            &format!("{:<width$}  {}", "Target", latest, width = LABEL_WIDTH),
            opts,
        ));
    }
    lines.push(guide_line(
        &rail,
        &format!("{:<width$}  {}", "Source", source, width = LABEL_WIDTH),
        opts,
    ));
    wrapped_command(&rail, command, opts, &mut lines);
    lines
}

fn guide_line(rail: &str, text: &str, _opts: RenderOptions) -> String {
    format!("{rail}  {text}")
}

/// The `Command` row, wrapped with a hanging indent so continuation lines read
/// as part of the command rather than as new rows.
fn wrapped_command(rail: &str, command: &str, opts: RenderOptions, lines: &mut Vec<String>) {
    // `render::wrap_text` hard-truncates any single word longer than the
    // budget, which would silently shorten a URL and make the command
    // uncopyable. Reserve enough width for the longest whitespace-separated
    // token so nothing has to be cut.
    let longest_word = command
        .split_whitespace()
        .map(|word| word.chars().count())
        .max()
        .unwrap_or(0);
    let budget = opts
        .width
        .saturating_sub(LABEL_WIDTH + 4)
        .max(longest_word)
        .max(24);
    let indent = format!("{:<width$}  ", "", width = LABEL_WIDTH);
    let mut chunks = crate::commands::render::wrap_text(command, budget);
    if chunks.is_empty() {
        return;
    }
    lines.push(guide_line(
        rail,
        &format!(
            "{:<width$}  {}",
            "Command",
            chunks.remove(0),
            width = LABEL_WIDTH
        ),
        opts,
    ));
    for chunk in chunks {
        lines.push(format!("{rail}  {indent}{chunk}"));
    }
}

/// One row per discovered install, the running one marked.
/// One row per discovered install.
///
/// `active` is the install `$PATH` resolves to — the one a bare `topos` runs —
/// which is marked with the cyan `❯` cursor. It is deliberately not "the
/// binary doing the reporting": running `topos` from a source checkout while
/// `$PATH` points at a Homebrew install would otherwise report the wrong one as
/// the one to upgrade.
pub(crate) fn install_rows(
    installs: &[Install],
    active: Option<&Path>,
    opts: RenderOptions,
) -> Vec<String> {
    let rail = crate::commands::render::guide('│', opts);
    let mut lines = Vec::new();
    let path_width = installs
        .iter()
        .map(|install| install.path.display().to_string().chars().count())
        .max()
        .unwrap_or(0)
        .min(opts.width.saturating_sub(32));
    for install in installs {
        let glyph = if active.is_some_and(|path| install.path == path) {
            paint("❯", Style::new().cyan(), opts)
        } else {
            " ".to_string()
        };
        let path = crate::commands::render::truncate_left(
            &install.path.display().to_string(),
            path_width.max(8),
        );
        lines.push(format!(
            "{rail}  {glyph} {path:<path_width$}  {:<7}  {}",
            install.version_label(),
            paint(install.channel.label(), Style::new().dim(), opts),
        ));
    }
    lines
}

/// The single-select: update now, or stay put.
pub(crate) fn action_step(latest: Option<&str>, current: &str) -> SelectStep {
    let label = match latest {
        Some(latest) => format!("Install update now ({latest})"),
        None => format!("Re-run the installer for {current}"),
    };
    let hint = match latest {
        Some(latest) => format!("{current} → {latest}"),
        None => "checksum-verified, in place".to_string(),
    };
    SelectStep {
        title: "".into(),
        keys: "↑↓ move · enter confirm · esc skip".into(),
        options: vec![
            SelectOption::new(label, hint),
            SelectOption::new(
                "Continue with current version",
                "re-run `topos update` later",
            ),
        ],
        initial: 0,
        layout: StepLayout::Question,
    }
}

/// The multi-install checkbox, one row per outdated install.
///
/// `topos install` uses a checkbox for the same reason: the question "which
/// harnesses" is genuinely plural, and "update one of several" is too.
pub(crate) fn install_menu(
    installs: &[Install],
    active: Option<&Path>,
    target: &str,
) -> Vec<crate::commands::menu::MenuOption> {
    installs
        .iter()
        .map(|install| {
            let is_active = active.is_some_and(|path| install.path == path);
            // Every row is the upgrade command, because with several installs
            // the channel — and therefore the command — differs per row.
            let hint = format!(
                "{} → {target} · {}",
                install.version_label(),
                install.channel.upgrade_command(),
            );
            let path = install.path.display().to_string();
            crate::commands::menu::MenuOption {
                // `id` is compared back against the path after the prompt
                // returns, and `name` is rendered; one leak serves both.
                id: Box::leak(path.clone().into_boxed_str()),
                name: Box::leak(path.into_boxed_str()),
                hint,
                hint_style: crate::commands::menu::HintStyle::Plain,
                checked: true,
                is_active,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use topos_mcp::update::Channel;

    use super::*;
    use std::path::PathBuf;

    fn opts() -> RenderOptions {
        RenderOptions {
            styled: false,
            width: 100,
        }
    }

    fn install(path: &str, version: &str, channel: Channel) -> Install {
        Install {
            path: PathBuf::from(path),
            channel,
            version: Some(version.to_string()),
        }
    }

    #[test]
    fn the_metadata_block_aligns_its_values() {
        let lines = metadata(
            "0.7.0",
            Some("0.7.1"),
            "binary install",
            "brew upgrade topos",
            opts(),
        );
        assert_eq!(
            lines,
            [
                "│  Current   0.7.0",
                "│  Target    0.7.1",
                "│  Source    binary install",
                "│  Command   brew upgrade topos",
            ]
        );
    }

    #[test]
    fn a_long_command_wraps_under_its_own_label_rather_than_being_cut() {
        let command = "TOPOS_UPDATE=1 curl -fsSL https://docs.krv.ai/topos/install.sh | bash";
        // Narrower than the command, so it must wrap.
        let narrow = RenderOptions {
            styled: false,
            width: 48,
        };
        let lines = metadata("0.7.0", Some("0.7.1"), "binary install", command, narrow);
        let tail: Vec<_> = lines
            .iter()
            .skip_while(|l| !l.contains("Command"))
            .collect();
        assert!(tail.len() > 1, "should have wrapped: {lines:?}");

        // Reassembling the wrapped rows must give the command back verbatim:
        // no truncation, no dropped token.
        let reassembled = tail
            .iter()
            .map(|line| line.trim_start().trim_start_matches('│').trim())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            reassembled.contains(command),
            "wrapped command must reassemble exactly:\n  got: {reassembled}\n  want: {command}"
        );
        assert!(
            lines.iter().all(|line| !line.contains('…')),
            "a command must never be truncated: {lines:?}"
        );
    }

    #[test]
    fn the_target_row_is_omitted_when_no_newer_release_is_known() {
        let lines = metadata("0.7.0", None, "binary install", "curl | bash", opts());
        assert!(!lines.iter().any(|l| l.contains("Target")), "{lines:?}");
    }

    #[test]
    fn the_path_first_install_is_marked_and_paths_stay_copyable() {
        let installs = vec![
            install("/opt/homebrew/bin/topos", "0.6.9", Channel::Homebrew),
            install("/home/dev/.local/bin/topos", "0.7.0", Channel::Binary),
        ];
        let lines = install_rows(
            &installs,
            Some(Path::new("/home/dev/.local/bin/topos")),
            opts(),
        );
        assert_eq!(lines.len(), 2);
        assert!(
            lines[0].starts_with("│    /opt/homebrew/bin/topos"),
            "{:?}",
            lines[0]
        );
        assert!(
            lines[1].contains('\u{276f}'),
            "the PATH-first install must be marked: {:?}",
            lines[1]
        );
        assert!(lines[0].contains("0.6.9") && lines[1].contains("0.7.0"));
    }

    /// A long path is shortened from the left: the tail is what identifies the
    /// file, and the leading directory repeats across installs.
    #[test]
    fn a_long_path_keeps_its_tail() {
        let installs = vec![install(
            "/Users/someone/very/deeply/nested/directory/structure/bin/topos",
            "0.7.0",
            Channel::Binary,
        )];
        let narrow = RenderOptions {
            styled: false,
            width: 60,
        };
        let lines = install_rows(&installs, Some(&installs[0].path), narrow);
        assert!(
            lines[0].contains("bin/topos"),
            "the identifying tail must survive: {:?}",
            lines[0]
        );
    }

    #[test]
    fn the_choice_names_the_target_version_on_both_rows() {
        let step = action_step(Some("0.7.1"), "0.7.0");
        assert_eq!(step.options[0].label, "Install update now (0.7.1)");
        assert_eq!(step.options[0].hint, "0.7.0 → 0.7.1");
        assert_eq!(step.options[1].label, "Continue with current version");
        assert!(step.options[1].hint.contains("topos update"));
    }

    #[test]
    fn an_unknown_latest_still_offers_the_reinstaller() {
        let step = action_step(None, "0.7.0");
        assert!(step.options[0].label.contains("0.7.0"));
        assert!(
            step.options[0].hint.contains("checksum"),
            "must say what the action guarantees: {:?}",
            step.options[0].hint
        );
    }

    /// Every channel topos will not touch must still name the command, since
    /// the user is left to run it themselves.
    #[test]
    fn an_unupgradable_channel_still_names_its_command() {
        for (channel, expected) in [
            (Channel::Source, "git pull && cargo build"),
            (Channel::Cargo, "cargo install topos"),
            (Channel::Python, "uv pip install"),
        ] {
            let command = channel.upgrade_command();
            assert!(
                command.contains(expected),
                "{} -> {command}",
                channel.label()
            );
            assert_eq!(
                channel.action(),
                super::super::Action::None,
                "{} must be advised, not run",
                channel.label()
            );
        }
    }
}
