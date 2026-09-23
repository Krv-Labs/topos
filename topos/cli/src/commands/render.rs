//! Human-readable rendering of a [`ClassificationResult`], shared by
//! `evaluate`'s per-file + rollup output.
//!
//! Split out of `evaluate.rs` -- printing is a separate concern from
//! `run`'s file-discovery/classification orchestration, and bundling
//! both pushed the file's cyclomatic total over the SIMPLE gate.

use console::{Style, Term};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use topos_engine::core::characteristic_morphism::ClassificationResult;
use topos_engine::core::omega::Generator;

#[derive(Clone, Copy)]
pub(crate) struct RenderOptions {
    pub(crate) styled: bool,
    pub(crate) width: usize,
}

impl RenderOptions {
    pub(crate) fn stdout() -> Self {
        let term = Term::stdout();
        Self::for_term(&term)
    }

    pub(crate) fn stderr() -> Self {
        let term = Term::stderr();
        Self::for_term(&term)
    }

    /// Width a piped or captured card is laid out for. `console` reports
    /// 80 columns when stdout is not a terminal, which truncates every table
    /// column; CI logs and pagers are comfortably wider than that.
    const PIPED_WIDTH: usize = 100;

    fn for_term(term: &Term) -> Self {
        let width = usize::from(term.size().1);
        let is_term = term.is_term();
        Self {
            styled: is_term && std::env::var_os("NO_COLOR").is_none(),
            width: if !is_term {
                Self::PIPED_WIDTH
            } else if width == 0 {
                120
            } else {
                width
            },
        }
    }
}

pub(crate) fn spinner(hidden: bool, message: &'static str) -> ProgressBar {
    if hidden {
        return ProgressBar::hidden();
    }
    let spinner = ProgressBar::new_spinner();
    spinner.set_draw_target(ProgressDrawTarget::stderr());
    spinner.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}").expect("static spinner template"),
    );
    spinner.set_message(message);
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));
    spinner
}

/// The per-file bar `evaluate` and `pr-recap` share, e.g. `Scoring ██▓░░
/// 3/13 gates.rs`. A single file finishes before a bar would read as
/// progress, so it is hidden then too.
pub(crate) fn progress_bar(label: &str, len: usize, hidden: bool) -> ProgressBar {
    if !bar_shown(len, hidden) {
        return ProgressBar::hidden();
    }
    let progress = ProgressBar::new(len as u64);
    progress.set_draw_target(ProgressDrawTarget::stderr());
    progress.set_style(
        ProgressStyle::with_template(&format!(
            "{label} {{bar:24.cyan/dim}} {{pos}}/{{len}} {{msg}}"
        ))
        .expect("static progress template")
        .progress_chars("█▓░"),
    );
    progress
}

fn bar_shown(len: usize, hidden: bool) -> bool {
    !hidden && len > 1
}

/// The unstyled text [`progress_bar`] draws at `pos` of `len`, rebuilt so
/// a fade can paint the same characters before and after indicatif does.
/// Mirrors indicatif's rounding: whole cells fill, one `▓` marks a
/// partial cell.
pub(crate) fn bar_line(label: &str, pos: u64, len: u64, msg: &str) -> String {
    const CELLS: usize = 24;
    let fill = if len == 0 {
        0.0
    } else {
        pos.min(len) as f32 / len as f32 * CELLS as f32
    };
    let filled = fill as usize;
    let head = usize::from(fill > 0.0 && filled < CELLS);
    format!(
        "{label} {}{}{} {pos}/{len} {msg}",
        "█".repeat(filled),
        "▓".repeat(head),
        "░".repeat(CELLS - filled - head)
    )
}

/// Framer-style opacity, proxied through the 256-color gray ramp: a block
/// leaving steps down from near-white, one arriving steps up to it.
pub(crate) const FADE_OUT: [u8; 6] = [252, 248, 244, 240, 237, 235];
pub(crate) const FADE_IN: [u8; 4] = [240, 244, 248, 252];
const FADE_STEP: std::time::Duration = std::time::Duration::from_millis(30);

/// Every frame of a fade: `rows` painted once per gray in `grays`.
pub(crate) fn fade_frames(rows: &[String], grays: &[u8]) -> Vec<Vec<String>> {
    grays
        .iter()
        .map(|&gray| {
            let style = Style::new().color256(gray).force_styling(true);
            rows.iter()
                .map(|row| style.apply_to(row).to_string())
                .collect()
        })
        .collect()
}

// A transient block is drawn from the cursor's row down, and between
// draws the cursor rests at the end of its last row, where indicatif
// leaves it too.

/// Make room for a block of `height` rows below the cursor.
pub(crate) fn open_block(term: &Term, height: usize) {
    let _ = term.write_str(&"\n".repeat(height.saturating_sub(1)));
}

/// Redraw an open block in place.
pub(crate) fn repaint(term: &Term, rows: &[String]) {
    let _ = term.write_str("\r");
    let _ = term.move_cursor_up(rows.len().saturating_sub(1));
    let _ = term.write_str(&format!("\x1b[2K{}", rows.join("\n\x1b[2K")));
    let _ = term.flush();
}

/// Fade an open block in through [`FADE_IN`]; the caller paints the final
/// styled text over the last frame. Unstyled output does not fade.
pub(crate) fn fade_in(term: &Term, rows: &[String], styled: bool) {
    if styled {
        play(term, &fade_frames(rows, &FADE_IN));
    }
}

/// Fade an open block out through [`FADE_OUT`], then clear it and leave
/// the cursor where the block started. Unstyled output clears at once.
pub(crate) fn fade_out(term: &Term, rows: &[String], styled: bool) {
    if styled {
        play(term, &fade_frames(rows, &FADE_OUT));
    }
    repaint(term, &vec![String::new(); rows.len()]);
    let _ = term.move_cursor_up(rows.len().saturating_sub(1));
    let _ = term.write_str("\r");
    let _ = term.flush();
}

fn play(term: &Term, frames: &[Vec<String>]) {
    for frame in frames {
        repaint(term, frame);
        std::thread::sleep(FADE_STEP);
    }
}

pub(crate) fn paint(text: impl ToString, style: Style, options: RenderOptions) -> String {
    let text = text.to_string();
    if options.styled {
        style.force_styling(true).apply_to(text).to_string()
    } else {
        text
    }
}

pub(crate) fn guide(value: char, options: RenderOptions) -> String {
    paint(value, Style::new().white(), options)
}

pub(crate) fn guide_line(text: impl ToString, style: Style, options: RenderOptions) -> String {
    format!("{}  {}", guide('│', options), paint(text, style, options))
}

pub(crate) fn print_lines(lines: impl IntoIterator<Item = String>) {
    for line in lines {
        println!("{line}");
    }
}

pub(crate) fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let word = truncate_right(word, width);
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > width {
            lines.push(current);
            current = String::new();
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(&word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

pub(crate) fn truncate_left(value: &str, width: usize) -> String {
    let count = value.chars().count();
    if count <= width {
        return value.to_string();
    }
    format!(
        "…{}",
        value.chars().skip(count - width + 1).collect::<String>()
    )
}

pub(crate) fn truncate_right(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    format!(
        "{}…",
        value
            .chars()
            .take(width.saturating_sub(1))
            .collect::<String>()
    )
}

/// Print a verdict, per-generator scores, and raw metrics for one result.
pub(crate) fn print_classification(result: &ClassificationResult) {
    if !result.is_parseable {
        println!("  {}", result.summary());
        return;
    }
    println!("  Verdict: {}", result.summary());
    for dim in Generator::ALL.map(Generator::as_str) {
        let Some(val) = result.dimensions.get(dim) else {
            continue;
        };
        let score = result.scores.get(dim).copied().unwrap_or(0.0) * 100.0;
        println!("    {dim}: {val} [{score:.0}%]");
    }
    if !result.raw_metrics.is_empty() {
        println!("  Raw metrics:");
        let mut keys: Vec<&String> = result.raw_metrics.keys().collect();
        keys.sort();
        for key in keys {
            let value = result.raw_metrics[key];
            println!("    {key}: {value:.3}");
        }
    }
}

/// Print only the diagnostic metrics for a compact single-file verbose run.
pub(crate) fn print_raw_metrics(result: &ClassificationResult) {
    if result.raw_metrics.is_empty() {
        return;
    }
    println!();
    println!("Raw metrics");
    let mut keys: Vec<&String> = result.raw_metrics.keys().collect();
    keys.sort();
    for key in keys {
        println!("  {key}: {:.3}", result.raw_metrics[key]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bar_needs_two_files_and_a_visible_run() {
        assert!(bar_shown(2, false));
        assert!(!bar_shown(1, false), "one file finishes before a bar reads");
        assert!(!bar_shown(0, false));
        assert!(!bar_shown(40, true), "--json never draws");
        assert!(progress_bar("Scoring", 1, false).is_hidden());
        assert!(progress_bar("Scoring", 40, true).is_hidden());
    }

    #[test]
    fn the_bar_line_matches_indicatifs_cells() {
        assert_eq!(
            bar_line("Scoring", 0, 13, "a.rs"),
            format!("Scoring {} 0/13 a.rs", "░".repeat(24))
        );
        // 24 * 5/13 = 9.2: nine whole cells, then the partial one.
        assert_eq!(
            bar_line("Scoring", 5, 13, "g.rs"),
            format!("Scoring {}▓{} 5/13 g.rs", "█".repeat(9), "░".repeat(14))
        );
        assert_eq!(
            bar_line("Evaluating", 13, 13, "z.rs"),
            format!("Evaluating {} 13/13 z.rs", "█".repeat(24))
        );
    }

    #[test]
    fn a_fade_paints_every_row_once_per_gray() {
        let rows = vec!["⠹  Tracing".to_string(), "   graphs 1/2".to_string()];
        let frames = fade_frames(&rows, &FADE_OUT);
        assert_eq!(frames.len(), FADE_OUT.len());
        for (frame, gray) in frames.iter().zip(FADE_OUT) {
            assert_eq!(frame.len(), rows.len());
            for (painted, row) in frame.iter().zip(&rows) {
                assert!(painted.contains(&format!("38;5;{gray}m")), "{painted:?}");
                assert_eq!(console::strip_ansi_codes(painted), row.as_str());
            }
        }
        let first = |grays: &[u8]| fade_frames(&rows, grays)[0][0].clone();
        assert!(first(&FADE_OUT).contains("38;5;252m"), "out starts bright");
        assert!(first(&FADE_IN).contains("38;5;240m"), "in starts faint");
        assert!(fade_frames(&rows, &[]).is_empty());
    }
}
