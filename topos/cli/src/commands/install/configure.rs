//! `topos install` — configure one or more agent harnesses to use Topos.
//!
//! One loop over the [`HARNESSES`](super::integrations::HARNESSES) table and,
//! inside it, one loop over that harness's artifacts. Adding a harness needs
//! no change here.

use std::path::Path;

use console::Style;

use super::integrations::{harness, Artifact, Outcome, State};
use crate::commands::render::{paint, RenderOptions};

fn ok(opts: RenderOptions) -> String {
    paint("✓", Style::new().green(), opts)
}

fn pending(opts: RenderOptions) -> String {
    paint("○", Style::new().color256(208), opts)
}

fn err(opts: RenderOptions) -> String {
    paint("✕", Style::new().red(), opts)
}

pub(crate) fn run(home: &Path, selected: &[String], dry_run: bool) -> Result<(), String> {
    let opts = RenderOptions::stdout();
    print_header(dry_run, opts);

    let mut success = true;
    for id in selected {
        success &= install_harness(id, home, dry_run, opts);
    }

    print_summary(success, opts)
}

fn install_harness(id: &str, home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    let Some(entry) = harness(id) else {
        println!("│  {} unknown harness: {id}", err(opts));
        return false;
    };
    println!("│  {}", paint(entry.label, Style::new().bold(), opts));
    let mut success = true;
    for artifact in entry.artifacts {
        success &= install_artifact(artifact, home, id, dry_run, opts);
    }
    println!("│");
    success
}

fn install_artifact(
    artifact: &Artifact,
    home: &Path,
    id: &str,
    dry_run: bool,
    opts: RenderOptions,
) -> bool {
    let what = artifact.describe(home);
    if artifact.state(home) == State::Active {
        println!("│    {} {what} already configured", ok(opts));
        return true;
    }
    if dry_run {
        println!("│    {} [dry run] would add {what}", pending(opts));
        return true;
    }
    match artifact.install(home, id) {
        Ok(Outcome::Changed) => {
            println!("│    {} added {what}", ok(opts));
            true
        }
        // Not `Active` beforehand yet nothing was written — report that
        // rather than claim a write that did not happen.
        Ok(_) => {
            println!("│    {} {what} already configured", ok(opts));
            true
        }
        Err(e) => {
            println!("│    {} {e}", err(opts));
            false
        }
    }
}

fn print_header(dry_run: bool, opts: RenderOptions) {
    let mode = if dry_run {
        " (DRY RUN — PREVIEW ONLY, NO CHANGES MADE)"
    } else {
        ""
    };
    println!(
        "{}",
        paint(
            format!("┌  Topos Harness Install{mode}"),
            Style::new().bold(),
            opts
        )
    );
    println!("│");
}

fn print_summary(success: bool, opts: RenderOptions) -> Result<(), String> {
    if success {
        println!("└  {}", paint("Done.", Style::new().bold(), opts));
        return Ok(());
    }
    println!(
        "└  {}",
        paint(
            "Incomplete — existing files were preserved; review the errors above.",
            Style::new().bold(),
            opts
        )
    );
    Err("one or more harnesses failed to configure".to_string())
}
