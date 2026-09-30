//! Column layout and glyph painting for the terminal card.
//!
//! Every line sits behind the `│  ` guide rail. Table rows are clamped to
//! the terminal before the marks are painted; prose (the verdict, the
//! findings, the meta line) wraps instead, so a path is never cut.

use console::Style;

use crate::commands::render::{guide, paint, truncate_right, wrap_text, RenderOptions};

/// Columns of the splits table: `SPLIT | PARENT → CHILDREN | MEDAL | <tail>`.
pub(super) const CHANGE_WIDTH: usize = 13;
pub(super) const FILE_WIDTH: usize = 31;
const MEDAL_WIDTH: usize = 18;
/// Offset where the splits-only columns begin.
pub(super) const TAIL: usize = CHANGE_WIDTH + FILE_WIDTH + MEDAL_WIDTH;
/// Content columns available after the `│  ` rail at the target width.
pub(super) const CONTENT: usize = 97;

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
    rail(&clamp(content, budget(options)), options)
}

/// `content` behind the rail with its marks painted, never clamped: the
/// caller has already fitted it, or wrapped it with [`wrapped`].
pub(super) fn rail(content: &str, options: RenderOptions) -> String {
    format!("{}  {}", guide('│', options), colorize(content, options))
}

/// `text` wrapped to the card behind the rail, `label` before the first
/// line and `continuation` before the rest.
pub(super) fn wrapped(
    label: &str,
    continuation: &str,
    text: &str,
    options: RenderOptions,
) -> Vec<String> {
    let mut plain = Vec::new();
    push_wrapped(&mut plain, label, continuation, text, budget(options));
    plain.iter().map(|text| rail(text, options)).collect()
}

/// [`wrapped`], dim: facts that must be read whole, like the meta line or
/// a directory, wrap instead of losing their tail.
pub(super) fn dim_wrapped(
    label: &str,
    continuation: &str,
    text: &str,
    options: RenderOptions,
) -> Vec<String> {
    let mut plain = Vec::new();
    push_wrapped(&mut plain, label, continuation, text, budget(options));
    plain.iter().map(|text| dim_line(text, options)).collect()
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

/// `CHANGE | FILE | MEDAL | <tail>`, trailing blanks trimmed.
pub(super) fn row(change: &str, file: &str, medal: &str, tail: &str) -> String {
    let mut out = pad(change, CHANGE_WIDTH);
    out.push_str(&pad(file, FILE_WIDTH));
    out.push_str(&pad(medal, MEDAL_WIDTH));
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
/// containing an `X` is never mistaken for a failure mark. Words, tier
/// names included, stay plain: a colored GOLD beside a plain BRONZE made
/// `GOLD → BRONZE` read as good news.
fn colorize(content: &str, options: RenderOptions) -> String {
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
            '↑' => out.push_str(&paint('↑', Style::new().green(), options)),
            '↓' => out.push_str(&paint('↓', Style::new().yellow(), options)),
            other => out.push(other),
        }
        index += 1;
    }
    out
}
