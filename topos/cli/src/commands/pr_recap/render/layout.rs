//! Column layout and glyph painting for the terminal card.
//!
//! Every row of both tables is `CHANGE | FILE | MEDAL | S C E N | <tail>`
//! at fixed widths; every line sits behind the `│  ` guide rail and is
//! clamped to the terminal before the marks are painted.

use console::Style;

use crate::commands::render::{guide, paint, truncate_right, wrap_text, RenderOptions};

pub(super) const CHANGE_WIDTH: usize = 13;
pub(super) const FILE_WIDTH: usize = 31;
const MEDAL_WIDTH: usize = 18;
/// One pillar cell: a dot plus an optional movement arrow.
pub(super) const PILLAR_COL: usize = 4;
const MATRIX_WIDTH: usize = PILLAR_COL * 4;
/// Offset where the splits-only columns begin.
pub(super) const TAIL: usize = CHANGE_WIDTH + FILE_WIDTH + MEDAL_WIDTH + MATRIX_WIDTH;
/// Content columns available after the `│  ` rail at the target width.
pub(super) const CONTENT: usize = 97;
pub(super) const MATRIX_HEADER: &str = "S   C   E   N";

pub(super) fn plain_budget(options: RenderOptions) -> usize {
    options.width.clamp(24, 100)
}

/// Content budget after the `│  ` rail.
pub(super) fn budget(options: RenderOptions) -> usize {
    plain_budget(options) - 3
}

pub(super) fn clamp(text: &str, width: usize) -> String {
    truncate_right(text, width)
}

pub(super) fn line(content: &str, options: RenderOptions) -> String {
    format!(
        "{}  {}",
        guide('│', options),
        colorize(&clamp(content, budget(options)), options)
    )
}

pub(super) fn header_dim_line(content: &str, options: RenderOptions) -> String {
    format!(
        "{}  {}",
        guide('│', options),
        paint(
            clamp(content, budget(options)),
            Style::new().bold().dim(),
            options
        )
    )
}

pub(super) fn dim_line(content: &str, options: RenderOptions) -> String {
    format!(
        "{}  {}",
        guide('│', options),
        paint(clamp(content, budget(options)), Style::new().dim(), options)
    )
}

/// Pad to `width` columns. Every glyph used on the card is single-width,
/// so a char count is a column count. Content that overruns its column
/// gets a single separator space rather than colliding with the next
/// one; content that fills it exactly is already flush.
pub(super) fn pad(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count > width {
        return format!("{text} ");
    }
    format!("{text}{}", " ".repeat(width - count))
}

/// `CHANGE | FILE | MEDAL | S C E N | <tail>`, trailing blanks trimmed.
pub(super) fn row(change: &str, file: &str, medal: &str, matrix: &str, tail: &str) -> String {
    let mut out = pad(change, CHANGE_WIDTH);
    out.push_str(&pad(file, FILE_WIDTH));
    out.push_str(&pad(medal, MEDAL_WIDTH));
    out.push_str(&pad(matrix, MATRIX_WIDTH));
    out.push_str(tail);
    out.trim_end().to_string()
}

/// Wrap `text` under `label`, with every line after the first aligned
/// under the first line's text rather than under the label.
pub(super) fn push_wrapped(
    lines: &mut Vec<String>,
    label: &str,
    continuation: &str,
    text: &str,
    width: usize,
) {
    let available = width.saturating_sub(label.chars().count()).max(12);
    for (index, chunk) in wrap_text(text, available).into_iter().enumerate() {
        let prefix = if index == 0 { label } else { continuation };
        lines.push(format!("{prefix}{chunk}"));
    }
}

/// Paint the meaning-carrying glyphs, leaving everything else plain.
///
/// `X` is only painted when it stands alone, so a path or an identifier
/// containing an `X` is never mistaken for a failure mark.
pub(crate) fn colorize(content: &str, options: RenderOptions) -> String {
    if !options.styled {
        return content.to_string();
    }
    let chars: Vec<char> = content.chars().collect();
    let mut out = String::with_capacity(content.len());
    let mut index = 0;
    while index < chars.len() {
        let glyph = chars[index];
        if glyph.is_ascii_uppercase() {
            let mut end = index;
            while end < chars.len() && (chars[end].is_ascii_uppercase() || chars[end] == '_') {
                end += 1;
            }
            let word: String = chars[index..end].iter().collect();
            if word == "X" {
                // A lone `X` is the failure mark (`X FAIL`, `X LOST`), not a word.
                out.push_str(&paint("X", Style::new().red().bold(), options));
            } else if let Some(style) = tier_style(&word) {
                out.push_str(&paint(&word, style, options));
            } else {
                out.push_str(&word);
            }
            index = end;
            continue;
        }
        let free = |position: Option<&char>| position.is_none_or(|c| !c.is_alphanumeric());
        let standalone =
            free(index.checked_sub(1).and_then(|p| chars.get(p))) && free(chars.get(index + 1));
        match glyph {
            '✓' => out.push_str(&paint('✓', Style::new().green().bold(), options)),
            'X' if standalone => out.push_str(&paint('X', Style::new().red().bold(), options)),
            '!' => out.push_str(&paint('!', Style::new().yellow().bold(), options)),
            '~' if standalone => out.push_str(&paint('~', Style::new().yellow().bold(), options)),
            '·' => out.push_str(&paint('·', Style::new().dim(), options)),
            '●' => out.push_str(&paint('●', Style::new().green(), options)),
            '○' => out.push_str(&paint('○', Style::new().red(), options)),
            '↑' => out.push_str(&paint('↑', Style::new().green(), options)),
            '↓' => out.push_str(&paint('↓', Style::new().yellow(), options)),
            other => out.push(other),
        }
        index += 1;
    }
    out
}

/// A tier word carries its own verdict, so it is coloured wherever it
/// appears — medal cell, tally or floor. Everything else uppercase
/// (`IMPROVEMENT`, `NAVIGABLE`, `SPLIT`) is left alone.
fn tier_style(word: &str) -> Option<Style> {
    match word {
        "SLOP" => Some(Style::new().red().bold()),
        "GOLD" | "PLATINUM" | "IDEAL" => Some(Style::new().green()),
        "SIMPLE_COMPOSABLE"
        | "SIMPLE_SECURE"
        | "COMPOSABLE_SECURE"
        | "SIMPLE_COMPOSABLE_SECURE"
        | "SIMPLE_NAVIGABLE"
        | "COMPOSABLE_NAVIGABLE"
        | "SIMPLE_COMPOSABLE_NAVIGABLE"
        | "SECURE_NAVIGABLE"
        | "SIMPLE_SECURE_NAVIGABLE"
        | "COMPOSABLE_SECURE_NAVIGABLE" => Some(Style::new().green().bold()),
        _ => None,
    }
}
