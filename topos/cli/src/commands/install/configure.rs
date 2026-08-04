//! `topos install` — register the Topos MCP server in the selected harnesses.
//!
//! A loop over the harness table with no per-harness branching: everything that
//! differs between harnesses lives in [`super::harness::HarnessSpec`], and
//! everything that differs between config formats lives in
//! [`super::artifact::Artifact`].

use std::path::Path;

use super::artifact::State;
use super::harness::{spec, HarnessSpec};
use super::report;
use super::state;
use crate::commands::render::RenderOptions;

pub(crate) fn run(
    home: &Path,
    binary: &Path,
    selected: &[String],
    dry_run: bool,
) -> Result<(), String> {
    let opts = RenderOptions::stdout();
    report::header("Topos Harness Install", dry_run, opts);
    println!("│  Using {}", binary.display());
    println!("│");

    let outcomes: Vec<bool> = selected
        .iter()
        .map(|id| configure_one(id, home, binary, dry_run, opts))
        .collect();

    finish(outcomes.iter().all(|ok| *ok), dry_run, opts)
}

fn configure_one(id: &str, home: &Path, binary: &Path, dry_run: bool, opts: RenderOptions) -> bool {
    let Some(harness) = spec(id) else {
        report::detail(&report::failed(opts), &format!("unknown harness: {id}"));
        return false;
    };
    report::harness_line(harness.name, opts);
    let success = configure_spec(harness, home, binary, dry_run, opts);
    if let Some(message) = (harness.note)(home) {
        report::note(&message, opts);
    }
    println!("│");
    success
}

fn configure_spec(
    harness: &HarnessSpec,
    home: &Path,
    binary: &Path,
    dry_run: bool,
    opts: RenderOptions,
) -> bool {
    let path = (harness.config_path)(home);
    let inspection = harness.artifact.inspect(&path, binary);
    match inspection.state {
        State::Active => {
            report::detail(&report::ok(opts), harness.active_msg);
            true
        }
        // A conflict needs a human decision. Reporting it and moving on is the
        // whole point: topos never rewrites a file it cannot account for.
        State::Conflict => {
            report::detail(
                &report::conflict(opts),
                &conflict_message(&inspection, &path),
            );
            false
        }
        _ if dry_run => {
            report::detail(&report::pending(opts), &preview_message(&inspection, &path));
            true
        }
        // `Incomplete` is drift: the entry is ours but its recorded command no
        // longer resolves, so this write is a repair rather than a first
        // registration.
        state => write_entry(
            harness,
            home,
            binary,
            &path,
            state == State::Incomplete,
            opts,
        ),
    }
}

fn conflict_message(inspection: &super::artifact::Inspection, path: &Path) -> String {
    inspection
        .detail
        .clone()
        .unwrap_or_else(|| format!("{} needs manual attention", path.display()))
}

fn preview_message(inspection: &super::artifact::Inspection, path: &Path) -> String {
    match &inspection.detail {
        Some(reason) => format!("[dry run] would repair {}: {reason}", path.display()),
        None => format!(
            "[dry run] would register the MCP server in {}",
            path.display()
        ),
    }
}

fn write_entry(
    harness: &HarnessSpec,
    home: &Path,
    binary: &Path,
    path: &Path,
    repair: bool,
    opts: RenderOptions,
) -> bool {
    match harness.artifact.apply(path, binary) {
        Ok(outcome) => {
            // Report what the write actually did rather than restating the
            // end state, so the output never claims a change that did not
            // happen — and so a repair reads as a repair.
            let wrote = outcome.is_some();
            record(harness.id, home, path, outcome);
            report::detail(&report::ok(opts), &applied_message(harness, repair, wrote));
            true
        }
        Err(message) => {
            report::detail(&report::failed(opts), &message);
            false
        }
    }
}

fn applied_message(harness: &HarnessSpec, repair: bool, wrote: bool) -> String {
    match (wrote, repair) {
        (false, _) => format!("{} (unchanged)", harness.active_msg),
        (true, true) => format!("repaired — {}", harness.active_msg),
        (true, false) => harness.active_msg.to_string(),
    }
}

/// Remember what this write brought into existence, so uninstall can undo
/// exactly that much and nothing more.
///
/// A failure to record is reported but does not fail the install: the entry is
/// already written and working, and the only cost is that uninstall will leave
/// an empty file or directory behind rather than something the user notices.
fn record(id: &str, home: &Path, path: &Path, outcome: Option<super::fsops::WriteOutcome>) {
    let Some(outcome) = outcome else {
        return;
    };
    if outcome.created_file {
        state::record_created_file(home, id, path).ok();
    }
    state::record_created_dirs(home, &outcome.created_dirs).ok();
}

fn finish(success: bool, dry_run: bool, opts: RenderOptions) -> Result<(), String> {
    if !success {
        report::footer(
            "Incomplete — existing files were preserved; review the entries above.",
            opts,
        );
        return Err("one or more harnesses could not be configured".to_string());
    }
    let message = if dry_run {
        "Done. (dry run — re-run without --dry-run to apply)"
    } else {
        "Done. Restart any running agent for it to pick up the new server."
    };
    report::footer(message, opts);
    Ok(())
}
