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
