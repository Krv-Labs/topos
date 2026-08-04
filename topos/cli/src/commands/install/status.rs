//! `topos status` (and its `topos install status` alias) — what is registered,
//! what needs repair, and what topos found but will not touch.
//!
//! The command exists so that a harness which silently fails to start the Topos
//! server has somewhere to say so. Three things therefore have to surface even
//! when they are inconvenient: path drift, conflicts topos refuses to resolve,
//! and [`super::residue`] left behind by an earlier draft or written by hand.

use std::path::Path;

use console::Style;
use serde_json::{json, Value};

use super::artifact::{Inspection, State};
use super::binary::resolve_binary_path;
use super::harness::{HarnessSpec, HARNESSES};
use super::report;
use super::residue::{self, Residue};
use crate::commands::render::{paint, RenderOptions};

pub(crate) fn run(home: &Path, json_output: bool) -> Result<(), String> {
    let binary = resolve_binary_path()?;
    let rows: Vec<(&HarnessSpec, Inspection)> = HARNESSES
        .iter()
        .map(|harness| {
            let inspection = harness
                .artifact
                .inspect(&(harness.config_path)(home), &binary);
            (harness, inspection)
        })
        .collect();
    let found = residue::scan(home, &binary);

    if json_output {
        return print_json(home, &binary, &rows, &found);
    }
    print_human(home, &binary, &rows, &found);
    Ok(())
}

fn print_human(home: &Path, binary: &Path, rows: &[(&HarnessSpec, Inspection)], found: &[Residue]) {
    let opts = RenderOptions::stdout();
    report::header("Topos Harness Status", false, opts);
    println!("│  Binary: {}", binary.display());
    println!("│");

    for (harness, inspection) in sorted(rows) {
        report::harness_line(harness.name, opts);
        report::detail(
            &report::glyph(inspection.state, opts),
            &describe(harness, inspection),
        );
        if let Some(message) = (harness.note)(home) {
            report::note(&message, opts);
        }
        println!("│");
    }

    print_residue(found, opts);
    let active = count(rows, State::Active);
    report::footer(
        &format!("{active}/{} harness integrations active.", rows.len()),
        opts,
    );
}

/// Active first, then anything needing attention, then the rest — the reader is
/// usually looking for the row that is wrong.
fn sorted<'a>(rows: &'a [(&'a HarnessSpec, Inspection)]) -> Vec<&'a (&'a HarnessSpec, Inspection)> {
    let mut ordered: Vec<&(&HarnessSpec, Inspection)> = rows.iter().collect();
    ordered.sort_by_key(|(_, inspection)| rank(inspection.state));
    ordered
}

fn rank(state: State) -> u8 {
    match state {
        State::Active => 0,
        State::Incomplete => 1,
        State::Conflict => 2,
        State::Absent => 3,
    }
}

/// The per-harness message: the spec's own wording for the settled states, and
/// the inspection's reason plus a repair instruction for the rest.
fn describe(harness: &HarnessSpec, inspection: &Inspection) -> String {
    match (inspection.state, &inspection.detail) {
        (State::Active, _) => harness.active_msg.to_string(),
        (State::Absent, _) => harness.absent_msg.to_string(),
        (State::Incomplete, Some(reason)) => {
            format!("{reason} — run `topos install {}`", harness.id)
        }
        (State::Incomplete, None) => format!("needs repair — run `topos install {}`", harness.id),
        (State::Conflict, Some(reason)) => reason.clone(),
        (State::Conflict, None) => "needs manual attention".to_string(),
    }
}

fn count(rows: &[(&HarnessSpec, Inspection)], state: State) -> usize {
    rows.iter()
        .filter(|(_, inspection)| inspection.state == state)
        .count()
}

fn print_residue(found: &[Residue], opts: RenderOptions) {
    if found.is_empty() {
        return;
    }
    report::harness_line("Found but not managed by topos", opts);
    for item in found {
        report::detail(
            &paint("▲", Style::new().color256(208), opts),
            &format!("{} — {}", item.path.display(), item.what),
        );
        println!("│      {}", paint(&item.advice, Style::new().dim(), opts));
    }
    println!("│");
}

fn print_json(
    home: &Path,
    binary: &Path,
    rows: &[(&HarnessSpec, Inspection)],
    found: &[Residue],
) -> Result<(), String> {
    let harnesses: Vec<Value> = rows
        .iter()
        .map(|(harness, inspection)| harness_json(home, harness, inspection))
        .collect();
    let residue: Vec<Value> = found
        .iter()
        .map(|item| {
            json!({
                "path": item.path.display().to_string(),
                "what": item.what,
                "advice": item.advice,
            })
        })
        .collect();
    let payload = json!({
        "binary": binary.display().to_string(),
        "active": count(rows, State::Active),
        "total": rows.len(),
        "harnesses": harnesses,
        "residue": residue,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn harness_json(home: &Path, harness: &HarnessSpec, inspection: &Inspection) -> Value {
    json!({
        "id": harness.id,
        "name": harness.name,
        "state": report::label(inspection.state),
        "config": (harness.config_path)(home).display().to_string(),
        "detail": inspection.detail,
        "note": (harness.note)(home),
    })
}
