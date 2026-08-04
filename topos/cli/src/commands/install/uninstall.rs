//! `topos uninstall` — remove Topos-owned entries from one or more agent
//! harnesses. Defaults to a dry-run preview; the caller decides whether to
//! actually apply (see `mod.rs`'s `--apply` handling).
//!
//! Mirrors `configure.rs`: one loop over the
//! [`HARNESSES`](super::integrations::HARNESSES) table, then one loop over
//! that harness's artifacts.

use std::path::Path;

use console::Style;

use super::edits::backup_path;
use super::integrations::{harness, Artifact, Outcome, State, HARNESSES};
use super::ownership::clear_created_files;
use crate::commands::render::{paint, RenderOptions};

fn removed(opts: RenderOptions) -> String {
    paint("●", Style::new().red(), opts)
}

fn absent(opts: RenderOptions) -> String {
    paint("○", Style::new().dim(), opts)
}

fn kept(opts: RenderOptions) -> String {
    paint("▲", Style::new().color256(208), opts)
}

fn err(opts: RenderOptions) -> String {
    paint("✕", Style::new().red(), opts)
}

pub(crate) fn run(
    home: &Path,
    selected: &[String],
    dry_run: bool,
    purge_backups: bool,
) -> Result<(), String> {
    let opts = RenderOptions::stdout();
    print_header(dry_run, opts);

    let mut success = true;
    for id in selected {
        success &= uninstall_harness(id, home, dry_run, opts);
    }

    if purge_backups && !dry_run {
        println!("│  Purging backup files...");
        purge_backup_files(home, opts);
        println!("│");
    }

    print_summary(success, dry_run, opts)
}

fn uninstall_harness(id: &str, home: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    let Some(entry) = harness(id) else {
        println!("│  {} unknown harness: {id}", err(opts));
        return false;
    };
    println!("│  {}", paint(entry.label, Style::new().bold(), opts));
    let mut success = true;
    for artifact in entry.artifacts {
        success &= remove_artifact(artifact, home, id, dry_run, opts);
    }
    if !dry_run {
        clear_created_files(home, id).ok();
    }
    println!("│");
    success
}

fn remove_artifact(
    artifact: &Artifact,
    home: &Path,
    id: &str,
    dry_run: bool,
    opts: RenderOptions,
) -> bool {
    let what = artifact.describe(home);
    if artifact.state(home) == State::Absent {
        println!("│    {} no {what}", absent(opts));
        return true;
    }
    if dry_run {
        println!("│    {} [dry run] would remove {what}", removed(opts));
        return true;
    }
    match artifact.remove(home, id, false) {
        Ok(Outcome::Changed) => {
            println!("│    {} removed {what}", removed(opts));
            true
        }
        Ok(Outcome::Preserved) => {
            println!("│    {} kept {what} — locally modified", kept(opts));
            true
        }
        Ok(Outcome::Unchanged) => {
            println!("│    {} no {what}", absent(opts));
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
            format!("┌  Topos Harness Uninstall{mode}"),
            Style::new().bold(),
            opts
        )
    );
    println!("│");
}

fn print_summary(success: bool, dry_run: bool, opts: RenderOptions) -> Result<(), String> {
    let summary = if dry_run {
        "Done. (dry run — re-run with --apply to make these changes)"
    } else {
        "Done."
    };
    println!("└  {}", paint(summary, Style::new().bold(), opts));
    if success {
        Ok(())
    } else {
        Err("one or more harnesses failed to uninstall cleanly".to_string())
    }
}

/// Covers every file any harness in the table touches, so a new harness's
/// backups are purged without a second list to remember to update.
fn purge_backup_files(home: &Path, opts: RenderOptions) {
    let backups = HARNESSES
        .iter()
        .flat_map(|entry| entry.artifacts)
        .flat_map(|artifact| artifact.paths(home))
        .map(|path| backup_path(&path));
    for backup in backups {
        if backup.is_file() && std::fs::remove_file(&backup).is_ok() {
            println!(
                "│    {} removed backup: {}",
                removed(opts),
                backup.display()
            );
        }
    }
}
