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
use super::skills_entry;
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
        // Its own line, never folded into the verdict above: a missing skill
        // must not mask a working MCP entry, nor the reverse.
        if harness.skill_ref {
            let (glyph, message) = skill_ref_line(home, opts);
            report::detail(&glyph, &message);
        }
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

/// The skill-reference line for a harness that has one. `None` from
/// [`skills_entry::inspect`] means there is no skill to point at, which reads
/// as absent with the install instruction rather than as a fault.
fn skill_ref_line(home: &Path, opts: RenderOptions) -> (String, String) {
    match skills_entry::inspect(home) {
        None => match skills_entry::skill_source(home) {
            // pi finds it unaided — that is the working end state, so it reads
            // as a tick rather than as a missing artifact.
            skills_entry::SkillSource::Discovered => (
                report::glyph(State::Active, opts),
                skills_entry::DISCOVERED_MSG.to_string(),
            ),
            _ => (report::absent(opts), skills_entry::NO_SKILL_MSG.to_string()),
        },
        Some(inspection) => {
            let message = match (inspection.state, inspection.detail) {
                (State::Active, _) => skills_entry::ACTIVE_MSG.to_string(),
                (State::Conflict, Some(reason)) => reason,
                (State::Conflict, None) => "needs manual attention".to_string(),
                _ => format!("{} — run `topos install pi`", skills_entry::ABSENT_MSG),
            };
            (report::glyph(inspection.state, opts), message)
        }
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
        "skillRef": harness.skill_ref.then(|| skill_ref_json(home)),
    })
}

/// `state` carries two values the [`State`] enum deliberately does not have,
/// because they describe someone else's artifact rather than ours:
/// `"discovered"` (pi finds the skill unaided, nothing to write) and
/// `"unavailable"` (no skill installed to point at).
fn skill_ref_json(home: &Path) -> Value {
    let state = match skills_entry::inspect(home) {
        Some(inspection) => report::label(inspection.state),
        None => match skills_entry::skill_source(home) {
            skills_entry::SkillSource::Discovered => "discovered",
            _ => "unavailable",
        },
    };
    json!({
        "state": state,
        "config": skills_entry::config_path(home).display().to_string(),
        "skillDir": skills_entry::skill_dir(home).map(|dir| dir.display().to_string()),
    })
}
