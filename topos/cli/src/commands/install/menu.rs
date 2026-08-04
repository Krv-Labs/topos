//! Interactive multi-select TTY menu for `topos install` / `topos
//! uninstall`, styled after the single-select loop in `commands/config.rs`
//! but with checkboxes instead of a single choice.

use console::{Key, Term};

use crate::commands::render::{paint, RenderOptions};

pub(crate) struct MenuOption {
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) hint: String,
    pub(crate) checked: bool,
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
        paint(format!("┌  {title}"), console::Style::new().bold(), opts),
        "│".to_string(),
        format!(
            "│  {}",
            paint(
                "↑↓ move · space toggle · a all · enter confirm · esc cancel",
                console::Style::new().dim(),
                opts,
            )
        ),
        "│".to_string(),
    ];
    for (idx, option) in options.iter().enumerate() {
        let pointer = if idx == cursor { "›" } else { " " };
        let checkbox = if option.checked { "●" } else { "○" };
        let row = format!("{pointer} {checkbox} {:<20} {}", option.name, option.hint);
        let style = if idx == cursor {
            console::Style::new().bold()
        } else if option.checked {
            console::Style::new()
        } else {
            console::Style::new().dim()
        };
        lines.push(format!("│ {}", paint(row, style, opts)));
    }
    lines.push("└".to_string());
    lines
}
