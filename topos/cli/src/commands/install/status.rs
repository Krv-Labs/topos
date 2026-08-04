//! `topos install status` — report which harnesses are configured.

use std::path::Path;

use console::Style;
use serde_json::json;

use super::integrations::{Harness, State, HARNESSES};
use crate::commands::render::{paint, RenderOptions};

pub(crate) fn run(home: &Path, json_output: bool) -> Result<(), String> {
    let mut states: Vec<(&Harness, State)> = HARNESSES
        .iter()
        .map(|entry| (entry, entry.state(home)))
        .collect();

    if json_output {
        return print_json(home, &states);
    }

    let opts = RenderOptions::stdout();
    println!(
        "{}",
        paint("┌  Topos Harness Status", Style::new().bold(), opts)
    );
    println!("│");

    states.sort_by_key(|(_, state)| rank(*state));
    for (idx, (entry, state)) in states.iter().enumerate() {
        println!("│  {}", paint(entry.name, Style::new().bold(), opts));
        println!("│    {}", describe_state(entry, *state, home, opts));
        if idx < states.len() - 1 {
            println!("│");
        }
    }

    let active = states.iter().filter(|(_, s)| *s == State::Active).count();
    println!("│");
    println!(
        "└  {}",
        paint(
            format!(
                "Done. {active}/{} harness integrations active.",
                HARNESSES.len()
            ),
            Style::new().bold(),
            opts,
        )
    );
    Ok(())
}

/// Absent splits in two: a harness that is installed here but unconfigured is
/// worth acting on, one that was never installed is not.
fn describe_state(entry: &Harness, state: State, home: &Path, opts: RenderOptions) -> String {
    match state {
        State::Active => format!("{} configured", paint("✓", Style::new().green(), opts)),
        State::Stale => format!(
            "{} stale or incomplete — run `topos install {}` to repair",
            paint("▲", Style::new().color256(208), opts),
            entry.id
        ),
        State::Absent if entry.is_detected(home) => format!(
            "{} detected, not configured — run `topos install {}`",
            paint("○", Style::new().color256(208), opts),
            entry.id
        ),
        State::Absent => format!(
            "{} not installed on this machine",
            paint("○", Style::new().dim(), opts)
        ),
    }
}

fn print_json(home: &Path, states: &[(&Harness, State)]) -> Result<(), String> {
    let active = states.iter().filter(|(_, s)| *s == State::Active).count();
    let harnesses: Vec<_> = states
        .iter()
        .map(|(entry, state)| {
            json!({
                "id": entry.id,
                "name": entry.name,
                "state": state_label(*state),
                "detected": entry.is_detected(home),
            })
        })
        .collect();
    let payload = json!({
        "active": active,
        "total": HARNESSES.len(),
        "harnesses": harnesses,
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?
    );
    Ok(())
}

fn rank(state: State) -> u8 {
    match state {
        State::Active => 0,
        State::Stale => 1,
        State::Absent => 2,
    }
}

fn state_label(state: State) -> &'static str {
    match state {
        State::Active => "active",
        State::Stale => "stale",
        State::Absent => "absent",
    }
}
