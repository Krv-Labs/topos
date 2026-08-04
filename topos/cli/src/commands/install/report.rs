//! The glyphs and line grammar shared by install, uninstall and status.
//!
//! Kept in one place so the three commands cannot drift apart visually, and so
//! each of them stays a loop over [`super::harness::HARNESSES`] with no
//! per-harness branching of its own.

use console::Style;

use super::artifact::State;
use crate::commands::render::{paint, RenderOptions};

pub(crate) fn ok(opts: RenderOptions) -> String {
    paint("✓", Style::new().green(), opts)
}

pub(crate) fn repair(opts: RenderOptions) -> String {
    paint("↻", Style::new().color256(208), opts)
}

pub(crate) fn conflict(opts: RenderOptions) -> String {
    paint("▲", Style::new().red(), opts)
}

pub(crate) fn absent(opts: RenderOptions) -> String {
    paint("○", Style::new().dim(), opts)
}

pub(crate) fn pending(opts: RenderOptions) -> String {
    paint("○", Style::new().color256(208), opts)
}

pub(crate) fn removed(opts: RenderOptions) -> String {
    paint("●", Style::new().red(), opts)
}

pub(crate) fn failed(opts: RenderOptions) -> String {
    paint("✕", Style::new().red(), opts)
}

pub(crate) fn glyph(state: State, opts: RenderOptions) -> String {
    match state {
        State::Active => ok(opts),
        State::Incomplete => repair(opts),
        State::Conflict => conflict(opts),
        State::Absent => absent(opts),
    }
}

pub(crate) fn label(state: State) -> &'static str {
    match state {
        State::Active => "active",
        State::Incomplete => "incomplete",
        State::Conflict => "conflict",
        State::Absent => "absent",
    }
}

pub(crate) fn header(title: &str, dry_run: bool, opts: RenderOptions) {
    let mode = if dry_run {
        " (DRY RUN — PREVIEW ONLY, NO CHANGES MADE)"
    } else {
        ""
    };
    println!(
        "{}",
        paint(format!("┌  {title}{mode}"), Style::new().bold(), opts)
    );
    println!("│");
}

pub(crate) fn harness_line(name: &str, opts: RenderOptions) {
    println!("│  {}", paint(name, Style::new().bold(), opts));
}

pub(crate) fn detail(glyph: &str, message: &str) {
    println!("│    {glyph} {message}");
}

/// A caveat that has to be seen even when the entry itself is fine — an
/// `Active` harness whose entry is about to be discarded is exactly the silent
/// failure this command exists to remove.
pub(crate) fn note(message: &str, opts: RenderOptions) {
    println!(
        "│    {} {}",
        paint("!", Style::new().yellow().bold(), opts),
        paint(message, Style::new().yellow(), opts)
    );
}

/// Closes the report. Callers already end each section with a bare `│`, so this
/// adds no separator of its own.
pub(crate) fn footer(message: &str, opts: RenderOptions) {
    println!("└  {}", paint(message, Style::new().bold(), opts));
}
