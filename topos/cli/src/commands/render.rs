//! Human-readable rendering of a [`ClassificationResult`], shared by
//! `evaluate`'s per-file + rollup output.
//!
//! Split out of `evaluate.rs` -- printing is a separate concern from
//! `run`'s file-discovery/classification orchestration, and bundling
//! both pushed the file's cyclomatic total over the SIMPLE gate.

use console::{Style, Term};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use topos_engine::core::characteristic_morphism::ClassificationResult;

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

    fn for_term(term: &Term) -> Self {
        let width = usize::from(term.size().1);
        Self {
            styled: term.is_term() && std::env::var_os("NO_COLOR").is_none(),
            width: if width == 0 { 120 } else { width },
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

fn truncate_right(value: &str, width: usize) -> String {
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
    for dim in ["simple", "composable", "secure"] {
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
