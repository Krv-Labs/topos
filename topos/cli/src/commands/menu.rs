//! Interactive TTY menus: the multi-select for `topos install` / `topos
//! uninstall`, their destructive-action confirm, and the single-select
//! steps of `topos config`. Styled after the kos wiki Clack-style
//! multi-select: colored radio glyphs, cyan cursor, and hint styling — not
//! whole-row paint.

use console::{Key, Style, Term};

use super::render::{paint, RenderOptions};

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

/// One choice in a [`SelectStep`].
pub(crate) struct SelectOption {
    pub(crate) label: &'static str,
    pub(crate) hint: String,
    /// The value the project uses today, marked `current`.
    pub(crate) current: bool,
    /// A letter that picks this choice at once (`y` / `n`), either case.
    pub(crate) key: Option<char>,
}

/// How a [`SelectStep`] sits under its header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StepLayout {
    /// A wizard step: a cyan title, a dim key hint and a blank rail above
    /// the choices, a bare `└` below. Ctrl-C interrupts the process.
    Wizard,
    /// A question asked mid-command, laid out like `topos install`: the
    /// header carries the title, a dim key hint sits between blank rails
    /// above the choices, each hint is parenthesized, and `└` closes bare.
    /// `title` is not drawn. Ctrl-C comes back as an error, so the command
    /// can exit with its own error code.
    Question,
}

/// One single-select screen: a cyan title, a dim key hint, then choices.
pub(crate) struct SelectStep {
    pub(crate) title: &'static str,
    pub(crate) keys: &'static str,
    pub(crate) options: Vec<SelectOption>,
    pub(crate) initial: usize,
    pub(crate) layout: StepLayout,
}

/// What a keypress means on a [`SelectStep`].
#[derive(Debug, PartialEq, Eq)]
enum SelectAction {
    Move(isize),
    /// Enter — take whatever the cursor is on.
    Accept,
    /// A choice's own letter.
    Pick(usize),
    Cancel,
    /// Ctrl-C on a [`StepLayout::Question`].
    Interrupt,
    Ignore,
}

fn interpret_select_key(key: Key, step: &SelectStep) -> SelectAction {
    if let Key::Char(typed) = key {
        let picked = step.options.iter().position(|option| {
            option
                .key
                .is_some_and(|letter| letter.eq_ignore_ascii_case(&typed))
        });
        if let Some(index) = picked {
            return SelectAction::Pick(index);
        }
    }
    if key == Key::CtrlC && step.layout == StepLayout::Question {
        return SelectAction::Interrupt;
    }
    match interpret_confirm_key(key) {
        ConfirmAction::Move(delta) => SelectAction::Move(delta),
        ConfirmAction::Accept => SelectAction::Accept,
        ConfirmAction::No => SelectAction::Cancel,
        ConfirmAction::Yes | ConfirmAction::Ignore => SelectAction::Ignore,
    }
}

/// Run one single-select step below `header` (the frame title and any
/// finished steps, redrawn on every keypress). Returns the chosen index, or
/// `None` if the user cancelled. A [`StepLayout::Question`] returns an
/// error on Ctrl-C. Clears everything it drew before returning, so the
/// caller can redraw the header with this step folded in.
pub(crate) fn run_select(header: &[String], step: &SelectStep) -> Result<Option<usize>, String> {
    let term = Term::stderr();
    let mut cursor = step.initial.min(step.options.len().saturating_sub(1));
    let mut rendered = 0usize;
    term.hide_cursor().map_err(|e| e.to_string())?;
    let result = (|| -> Result<Option<usize>, String> {
        loop {
            if rendered > 0 {
                term.clear_last_lines(rendered).map_err(|e| e.to_string())?;
            }
            let lines = render_select(header, step, cursor, RenderOptions::stderr());
            rendered = lines.len();
            for line in &lines {
                term.write_line(line).map_err(|e| e.to_string())?;
            }
            // `read_key` raises SIGINT on Ctrl-C; the raw read hands it back
            // as a key, which only a question turns into an error.
            let key = match step.layout {
                StepLayout::Wizard => term.read_key(),
                StepLayout::Question => term.read_key_raw(),
            };
            match interpret_select_key(key.map_err(|e| e.to_string())?, step) {
                SelectAction::Move(delta) => {
                    cursor = move_cursor(cursor, delta, step.options.len())
                }
                SelectAction::Accept => return Ok(Some(cursor)),
                SelectAction::Pick(index) => return Ok(Some(index)),
                SelectAction::Cancel => return Ok(None),
                SelectAction::Interrupt => return Err("interrupted at the prompt".to_string()),
                SelectAction::Ignore => {}
            }
        }
    })();
    term.clear_last_lines(rendered).ok();
    term.show_cursor().ok();
    result
}

pub(crate) fn render_select(
    header: &[String],
    step: &SelectStep,
    cursor: usize,
    opts: RenderOptions,
) -> Vec<String> {
    let mut lines = header.to_vec();
    lines.push(match step.layout {
        StepLayout::Wizard => format!("│  {}", paint(step.title, Style::new().cyan().bold(), opts)),
        StepLayout::Question => "│".to_string(),
    });
    lines.push(format!("│  {}", paint(step.keys, Style::new().dim(), opts)));
    lines.push("│".to_string());
    let width = step
        .options
        .iter()
        .map(|o| o.label.chars().count())
        .max()
        .unwrap_or(0);
    for (idx, option) in step.options.iter().enumerate() {
        if option.hint.is_empty() && !option.current {
            lines.push(format!(
                "│ {}",
                choice_row(option.label, idx == cursor, opts)
            ));
            continue;
        }
        let label = format!("{:<width$}", option.label);
        let mut hint = option.hint.clone();
        if option.current {
            hint.push_str(" · current");
        }
        if step.layout == StepLayout::Question {
            hint = format!("({hint})");
        }
        lines.push(format!(
            "│ {}   {}",
            choice_row(&label, idx == cursor, opts),
            paint(hint, Style::new().dim(), opts)
        ));
    }
    lines.push("└".to_string());
    lines
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
        lines.push(format!("│ {}", choice_row(label, idx == cursor, opts)));
    }
    lines.push("│".to_string());
    lines.push(format!(
        "│  {}",
        paint("↑↓ · enter · esc", Style::new().dim(), opts)
    ));
    lines.push("└".to_string());
    lines
}

/// Cursor, radio and label of one single-select row.
fn choice_row(label: &str, is_cursor: bool, opts: RenderOptions) -> String {
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

    #[test]
    fn select_marks_cursor_and_current_below_the_header() {
        let step = SelectStep {
            title: "PR gate",
            keys: "↑↓ move · enter save · esc cancel",
            options: vec![
                SelectOption {
                    label: "Recommended",
                    hint: "the default".into(),
                    current: true,
                    key: None,
                },
                SelectOption {
                    label: "Strict",
                    hint: "warnings fail too".into(),
                    current: false,
                    key: None,
                },
            ],
            initial: 0,
            layout: StepLayout::Wizard,
        };
        let header = vec!["┌  Topos project settings".to_string(), "│".to_string()];
        let lines = render_select(&header, &step, 1, opts());
        assert_eq!(lines[0], "┌  Topos project settings");
        assert_eq!(lines[2], "│  PR gate");
        let recommended = lines.iter().find(|l| l.contains("Recommended")).unwrap();
        let strict = lines.iter().find(|l| l.contains("Strict")).unwrap();
        assert!(recommended.contains('○') && recommended.contains("the default · current"));
        assert!(strict.contains('❯') && strict.contains('●'), "{strict}");
        // Labels are padded so the hints line up. Compare columns, not byte
        // offsets: the `❯` cursor is wider in UTF-8 than the blank it replaces.
        let column = |line: &str, needle: &str| line[..line.find(needle).unwrap()].chars().count();
        assert_eq!(
            column(recommended, "the default"),
            column(strict, "warnings fail too")
        );
        assert_eq!(lines.last().unwrap(), "└");
    }

    fn yes_no(layout: StepLayout) -> SelectStep {
        let option = |label, hint: &str, key| SelectOption {
            label,
            hint: hint.to_string(),
            current: false,
            key,
        };
        SelectStep {
            title: "Unused by a question",
            keys: "↑↓ move · y/n pick · enter confirm · esc skip",
            options: vec![
                option("Yes", "build it", Some('y')),
                option("No", "", Some('n')),
            ],
            initial: 0,
            layout,
        }
    }

    #[test]
    fn a_question_puts_its_keys_above_the_choices_like_install() {
        let header = vec!["┌  Build it?".to_string()];
        let lines = render_select(&header, &yes_no(StepLayout::Question), 0, opts());
        assert_eq!(
            lines,
            [
                "┌  Build it?",
                "│",
                "│  ↑↓ move · y/n pick · enter confirm · esc skip",
                "│",
                "│ ❯ ● Yes   (build it)",
                "│   ○ No",
                "└",
            ]
        );
    }

    #[test]
    fn option_letters_pick_and_ctrl_c_interrupts_only_a_question() {
        let question = yes_no(StepLayout::Question);
        assert_eq!(
            interpret_select_key(Key::Char('Y'), &question),
            SelectAction::Pick(0)
        );
        assert_eq!(
            interpret_select_key(Key::Char('n'), &question),
            SelectAction::Pick(1)
        );
        assert_eq!(
            interpret_select_key(Key::Escape, &question),
            SelectAction::Cancel
        );
        assert_eq!(
            interpret_select_key(Key::CtrlC, &question),
            SelectAction::Interrupt
        );
        assert_eq!(
            interpret_select_key(Key::Enter, &question),
            SelectAction::Accept
        );

        // A wizard step without letters keeps its old keys: `n` and Ctrl-C
        // cancel, `y` does nothing.
        let mut wizard = yes_no(StepLayout::Wizard);
        for option in &mut wizard.options {
            option.key = None;
        }
        assert_eq!(
            interpret_select_key(Key::Char('n'), &wizard),
            SelectAction::Cancel
        );
        assert_eq!(
            interpret_select_key(Key::CtrlC, &wizard),
            SelectAction::Cancel
        );
        assert_eq!(
            interpret_select_key(Key::Char('y'), &wizard),
            SelectAction::Ignore
        );
    }
}
