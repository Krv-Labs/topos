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

const WORKING_FRAMES: [char; 6] = ['⠿', '⠛', '⠹', '⠼', '⠶', '⠦'];
const WORKING_HOLD: std::time::Duration = std::time::Duration::from_secs(7);
const WORKING_LINES: &[&str] = &[
    "Reading the two trees...",
    "Scoring the files that actually changed",
    "Checking which gates still hold...",
    "Separating a medal move from a score dip",
    "Looking for a call that was not there before...",
    "Leaving unmeasured coupling unmeasured",
];

/// A stderr working line for a command long enough to look hung.
///
/// The frame turns continuously. The sentence types in, holds for seven
/// seconds, deletes itself one character at a time, waits half a second,
/// and the next sentence types in. A fast command drops the line before
/// the card, so a quick run never flashes it.
pub(crate) struct Working {
    shown: bool,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Working {
    pub(crate) fn start() -> Self {
        let term = Term::stderr();
        if !term.is_term() || std::env::var_os("NO_COLOR").is_some() {
            return Self {
                shown: false,
                stop: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true)),
                thread: None,
            };
        }
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&stop);
        let thread = std::thread::spawn(move || working_loop(&term, &flag));
        Self {
            shown: true,
            stop,
            thread: Some(thread),
        }
    }

    pub(crate) fn clear(mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if self.shown {
            let _ = Term::stderr().clear_line();
            let _ = Term::stderr().write_str("\r");
        }
        self.shown = false;
    }
}

impl Drop for Working {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn working_loop(term: &Term, stop: &std::sync::atomic::AtomicBool) {
    // Three times the old 80ms character step. The spinner still moves
    // on this tick, so it turns faster too.
    let tick = std::time::Duration::from_millis(27);
    let gap = std::time::Duration::from_millis(500);
    let mut frame = 0usize;
    let mut line = 0usize;
    let mut shown = 0usize;
    let mut deleting = false;
    let mut hold_started = std::time::Instant::now();
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        let text = WORKING_LINES[line];
        let count = text.chars().count();
        let visible: String = text.chars().take(shown).collect();
        let painted = Style::new().dim().force_styling(true).apply_to(format!(
            "{}  {visible}",
            WORKING_FRAMES[frame % WORKING_FRAMES.len()]
        ));
        let _ = term.clear_line();
        let _ = term.write_str(&format!("\r{painted}"));
        let _ = term.flush();
        frame += 1;
        if !deleting && shown < count {
            shown += 1;
            if shown == count {
                hold_started = std::time::Instant::now();
            }
        } else if !deleting && hold_started.elapsed() >= WORKING_HOLD {
            deleting = true;
        } else if deleting && shown > 0 {
            shown -= 1;
        } else if deleting {
            deleting = false;
            line = (line + 1) % WORKING_LINES.len();
            std::thread::sleep(gap);
            continue;
        }
        std::thread::sleep(tick);
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
