//! Interactive multi-select TTY menu for `topos install` / `topos
//! uninstall`, styled after the kos wiki Clack-style multi-select:
//! colored radio glyphs, cyan cursor, and hint styling — not whole-row
//! paint.

use console::{Key, Style, Term};

use crate::commands::render::{paint, RenderOptions};

/// How the trailing hint should be painted. Plain text stays in
/// [`MenuOption::hint`]; the style decides the glyph + color wrapper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HintStyle {
    /// Green `✓ active`.
    Active,
    /// Orange `▲ needs repair` (or similar attention copy).
    Repair,
    /// Dim unadorned text (`detected`, `not configured`, conflict copy).
    Plain,
}

pub(crate) struct MenuOption {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    /// Unstyled hint body (no leading glyph).
    pub(crate) hint: String,
    pub(crate) hint_style: HintStyle,
    pub(crate) checked: bool,
    /// When checked, paint the radio blue (already integrated) instead of green.
    pub(crate) is_active: bool,
}

/// What a keypress means, independent of terminal/IO concerns — kept
/// separate from `run_menu`'s loop so neither function's branching stacks
/// on top of the other's.
enum Action {
    Move(isize),
    Toggle,
    ToggleAll,
    Confirm,
    Cancel,
    Ignore,
}

fn interpret_key(key: Key) -> Action {
    match key {
        Key::ArrowUp | Key::Char('k') => Action::Move(-1),
        Key::ArrowDown | Key::Char('j') => Action::Move(1),
        Key::Char(' ') => Action::Toggle,
        Key::Char('a') => Action::ToggleAll,
        Key::Enter => Action::Confirm,
        Key::Escape | Key::CtrlC | Key::Char('q') => Action::Cancel,
        _ => Action::Ignore,
    }
}

fn move_cursor(cursor: usize, delta: isize, len: usize) -> usize {
    if delta < 0 {
        cursor.checked_sub(1).unwrap_or(len - 1)
    } else {
        (cursor + 1) % len
    }
}

fn toggle_all(options: &mut [MenuOption]) {
    let all_checked = options.iter().all(|o| o.checked);
    for option in options {
        option.checked = !all_checked;
    }
}

fn confirmed_selection(options: &[MenuOption]) -> Vec<String> {
    options
        .iter()
        .filter(|o| o.checked)
        .map(|o| o.id.to_string())
        .collect()
}

/// Run an interactive checkbox list. Returns the selected ids, or `None` if
/// the user cancelled (Esc/q/Ctrl-C).
pub(crate) fn run_menu(
    title: &str,
    mut options: Vec<MenuOption>,
) -> Result<Option<Vec<String>>, String> {
    if options.is_empty() {
        return Ok(Some(Vec::new()));
    }
    let term = Term::stderr();
    let mut cursor = 0usize;
    let mut rendered = 0usize;
    term.hide_cursor().map_err(|e| e.to_string())?;
    let result = (|| -> Result<Option<Vec<String>>, String> {
        loop {
            if rendered > 0 {
                term.clear_last_lines(rendered).map_err(|e| e.to_string())?;
            }
            let lines = render(title, &options, cursor, RenderOptions::stderr());
            rendered = lines.len();
            for line in &lines {
                term.write_line(line).map_err(|e| e.to_string())?;
            }
            match interpret_key(term.read_key().map_err(|e| e.to_string())?) {
                Action::Move(delta) => cursor = move_cursor(cursor, delta, options.len()),
                Action::Toggle => options[cursor].checked = !options[cursor].checked,
                Action::ToggleAll => toggle_all(&mut options),
                Action::Confirm => return Ok(Some(confirmed_selection(&options))),
                Action::Cancel => return Ok(None),
                Action::Ignore => {}
            }
        }
    })();
    term.show_cursor().ok();
    result
}

/// Destructive-action confirm: plan lines + single-select with **No** on top
/// and pre-selected so Enter alone aborts. Arrow down to **Yes**. Esc/q = No.
///
/// Returns `true` only when the user explicitly selects Yes.
pub(crate) fn run_confirm(title: &str, plan: &[String]) -> Result<bool, String> {
    let term = Term::stderr();
    // Index 0 = No (safe default). Index 1 = Yes.
    let mut cursor = 0usize;
    let mut rendered = 0usize;
    term.hide_cursor().map_err(|e| e.to_string())?;
    let result = (|| -> Result<bool, String> {
        loop {
            if rendered > 0 {
                term.clear_last_lines(rendered).map_err(|e| e.to_string())?;
            }
            let lines = render_confirm(title, plan, cursor, RenderOptions::stderr());
            rendered = lines.len();
            for line in &lines {
                term.write_line(line).map_err(|e| e.to_string())?;
            }
            match interpret_confirm_key(term.read_key().map_err(|e| e.to_string())?) {
                ConfirmAction::Move(delta) => cursor = move_cursor(cursor, delta, 2),
                ConfirmAction::Accept => return Ok(cursor == 1),
                ConfirmAction::Yes => return Ok(true),
                ConfirmAction::No => return Ok(false),
                ConfirmAction::Ignore => {}
            }
        }
    })();
    term.show_cursor().ok();
    result
}

enum ConfirmAction {
    Move(isize),
    /// Enter — take whatever the cursor is on.
    Accept,
    Yes,
    No,
    Ignore,
}

fn interpret_confirm_key(key: Key) -> ConfirmAction {
    match key {
        Key::ArrowUp | Key::Char('k') => ConfirmAction::Move(-1),
        Key::ArrowDown | Key::Char('j') => ConfirmAction::Move(1),
        Key::Enter => ConfirmAction::Accept,
        Key::Char('y' | 'Y') => ConfirmAction::Yes,
        Key::Char('n' | 'N') | Key::Escape | Key::CtrlC | Key::Char('q') => ConfirmAction::No,
        _ => ConfirmAction::Ignore,
    }
}

fn render_confirm(title: &str, plan: &[String], cursor: usize, opts: RenderOptions) -> Vec<String> {
    let choices = ["No", "Yes"];
    let mut lines = vec![
        paint(format!("┌  {title}"), Style::new().bold(), opts),
        "│".to_string(),
    ];
    if plan.is_empty() {
        lines.push(format!(
            "│  {}",
            paint("nothing to change", Style::new().dim(), opts)
        ));
    } else {
        for item in plan {
            lines.push(format!(
                "│  {} {}",
                paint("·", Style::new().dim(), opts),
                item
            ));
        }
    }
    lines.push("│".to_string());
    for (idx, label) in choices.iter().enumerate() {
        lines.push(format!(
            "│ {}",
            render_confirm_row(label, idx == cursor, opts)
        ));
    }
    lines.push("│".to_string());
    lines.push(format!(
        "│  {}",
        paint("↑↓ · enter · esc", Style::new().dim(), opts)
    ));
    lines.push("└".to_string());
    lines
}

fn render_confirm_row(label: &str, is_cursor: bool, opts: RenderOptions) -> String {
    let pointer = if is_cursor {
        paint("❯", Style::new().cyan(), opts)
    } else {
        " ".to_string()
    };
    let radio = if is_cursor {
        paint("●", Style::new().green(), opts)
    } else {
        paint("○", Style::new().dim(), opts)
    };
    let text = if is_cursor {
        paint(label, Style::new().bold(), opts)
    } else {
        label.to_string()
    };
    format!("{pointer} {radio} {text}")
}

fn render(title: &str, options: &[MenuOption], cursor: usize, opts: RenderOptions) -> Vec<String> {
    let mut lines = vec![
        paint(format!("┌  {title}"), Style::new().bold(), opts),
        "│".to_string(),
        format!(
            "│  {}",
            paint(
                "↑↓ move · space toggle · a all · enter confirm · esc cancel",
                Style::new().dim(),
                opts,
            )
        ),
        "│".to_string(),
    ];
    for (idx, option) in options.iter().enumerate() {
        lines.push(format!("│ {}", render_row(option, idx == cursor, opts)));
    }
    lines.push("└".to_string());
    lines
}

/// Radio + cursor take the color; the label is bold only under the cursor.
fn render_row(option: &MenuOption, is_cursor: bool, opts: RenderOptions) -> String {
    let pointer = if is_cursor {
        paint("❯", Style::new().cyan(), opts)
    } else {
        " ".to_string()
    };
    let radio = radio_glyph(option.checked, option.is_active, opts);
    let name = format!("{:<20}", option.name);
    let label = if is_cursor {
        paint(name, Style::new().bold(), opts)
    } else {
        name
    };
    let hint = format_hint(option, opts);
    format!("{pointer} {radio} {label}{hint}")
}

fn radio_glyph(checked: bool, is_active: bool, opts: RenderOptions) -> String {
    if checked {
        // Blue when already integrated (kos `is_active`); green when newly selected.
        if is_active {
            paint("●", Style::new().color256(39), opts)
        } else {
            paint("●", Style::new().green(), opts)
        }
    } else {
        paint("○", Style::new().dim(), opts)
    }
}

fn format_hint(option: &MenuOption, opts: RenderOptions) -> String {
    if option.hint.is_empty() {
        return String::new();
    }
    let body = match option.hint_style {
        HintStyle::Active => paint(format!("✓ {}", option.hint), Style::new().green(), opts),
        HintStyle::Repair => paint(
            format!("▲ {}", option.hint),
            Style::new().color256(208),
            opts,
        ),
        HintStyle::Plain => paint(&option.hint, Style::new().dim(), opts),
    };
    format!(" ({body})")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> RenderOptions {
        RenderOptions {
            styled: false,
            width: 80,
        }
    }

    #[test]
    fn confirm_shows_plan_then_no_default() {
        let plan = vec!["Codex CLI — remove MCP entry from ~/.codex/config.toml".into()];
        let lines = render_confirm("Uninstall Topos from these agents?", &plan, 0, opts());
        let joined = lines.join("\n");
        assert!(
            joined.contains("Uninstall Topos from these agents?"),
            "{joined}"
        );
        assert!(joined.contains("Codex CLI — remove MCP entry"), "{joined}");
        assert!(
            !joined.contains("dry run") && !joined.contains("DRY RUN"),
            "confirm is the real plan, not a dry-run report: {joined}"
        );
        // No is first choice; with cursor 0 the filled radio sits on No.
        let no_idx = joined.find("\n│ ❯ ● No").or_else(|| joined.find("No"));
        let yes_idx = joined.find("Yes");
        assert!(no_idx.is_some() && yes_idx.is_some());
        assert!(
            no_idx.unwrap() < yes_idx.unwrap(),
            "No must be listed above Yes"
        );
        let no_line = lines.iter().find(|l| l.contains("No")).unwrap();
        let yes_line = lines.iter().find(|l| l.contains("Yes")).unwrap();
        assert!(
            no_line.contains('●'),
            "No should be selected by default: {no_line}"
        );
        assert!(
            yes_line.contains('○'),
            "Yes should be unselected by default: {yes_line}"
        );
        assert!(no_line.contains('❯'), "cursor on No: {no_line}");
        // Plan comes before the Yes/No rows.
        let plan_idx = joined.find("Codex CLI").unwrap();
        assert!(plan_idx < no_idx.unwrap());
    }

    #[test]
    fn confirm_yes_row_selected_when_cursor_moves_down() {
        let lines = render_confirm("Uninstall?", &[], 1, opts());
        let yes_line = lines.iter().find(|l| l.contains("Yes")).unwrap();
        let no_line = lines.iter().find(|l| l.contains("No")).unwrap();
        assert!(
            yes_line.contains('●') && yes_line.contains('❯'),
            "{yes_line}"
        );
        assert!(no_line.contains('○'), "{no_line}");
        assert!(
            lines.iter().any(|l| l.contains("nothing to change")),
            "empty plan should say so"
        );
    }

    #[test]
    fn confirm_keys_map_safely() {
        assert!(matches!(
            interpret_confirm_key(Key::Enter),
            ConfirmAction::Accept
        ));
        assert!(matches!(
            interpret_confirm_key(Key::Char('y')),
            ConfirmAction::Yes
        ));
        assert!(matches!(
            interpret_confirm_key(Key::Char('n')),
            ConfirmAction::No
        ));
        assert!(matches!(
            interpret_confirm_key(Key::Escape),
            ConfirmAction::No
        ));
        assert!(matches!(
            interpret_confirm_key(Key::ArrowDown),
            ConfirmAction::Move(1)
        ));
        assert!(matches!(
            interpret_confirm_key(Key::ArrowUp),
            ConfirmAction::Move(-1)
        ));
    }

    #[test]
    fn active_checked_radio_is_filled_dot() {
        let row = render_row(
            &MenuOption {
                id: "claude",
                name: "Claude Code",
                hint: "active".into(),
                hint_style: HintStyle::Active,
                checked: true,
                is_active: true,
            },
            false,
            opts(),
        );
        assert!(row.contains('●'), "{row}");
        assert!(row.contains("✓ active"), "{row}");
    }

    #[test]
    fn selected_but_not_active_still_filled() {
        let row = render_row(
            &MenuOption {
                id: "codex",
                name: "Codex",
                hint: "detected".into(),
                hint_style: HintStyle::Plain,
                checked: true,
                is_active: false,
            },
            false,
            opts(),
        );
        assert!(row.contains('●'), "{row}");
        assert!(row.contains("detected"), "{row}");
    }

    #[test]
    fn cursor_uses_clack_chevron() {
        let row = render_row(
            &MenuOption {
                id: "gemini",
                name: "Gemini CLI",
                hint: "not configured".into(),
                hint_style: HintStyle::Plain,
                checked: false,
                is_active: false,
            },
            true,
            opts(),
        );
        assert!(row.contains('❯'), "{row}");
        assert!(row.contains('○'), "{row}");
    }

    #[test]
    fn repair_hint_gets_triangle_prefix() {
        let row = render_row(
            &MenuOption {
                id: "cursor",
                name: "Cursor",
                hint: "needs repair".into(),
                hint_style: HintStyle::Repair,
                checked: true,
                is_active: false,
            },
            false,
            opts(),
        );
        assert!(row.contains("▲ needs repair"), "{row}");
    }

    #[test]
    fn styled_radios_distinguish_active_blue_from_selected_green() {
        let styled = RenderOptions {
            styled: true,
            width: 80,
        };
        let active = render_row(
            &MenuOption {
                id: "claude",
                name: "Claude Code",
                hint: "active".into(),
                hint_style: HintStyle::Active,
                checked: true,
                is_active: true,
            },
            false,
            styled,
        );
        let selected = render_row(
            &MenuOption {
                id: "codex",
                name: "Codex",
                hint: "detected".into(),
                hint_style: HintStyle::Plain,
                checked: true,
                is_active: false,
            },
            true,
            styled,
        );
        // 256-color blue (39) vs standard green — same glyph, different paint.
        assert!(
            active.contains("38;5;39") || active.contains("38:5:39"),
            "active radio should be blue-ish: {active:?}"
        );
        assert!(
            selected.contains("32m") || selected.contains("32;"),
            "selected non-active radio should be green: {selected:?}"
        );
        assert!(
            selected.contains("36m") || selected.contains("36;"),
            "cursor should be cyan: {selected:?}"
        );
    }
}
