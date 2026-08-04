//! `topos install status` — report which harnesses are configured.

use std::path::Path;

use console::Style;
use serde_json::json;

use super::integrations::{harness_name, integration_state, State, SUPPORTED};
use crate::commands::render::{paint, RenderOptions};

pub(crate) fn run(home: &Path, json_output: bool) -> Result<(), String> {
    let states: Vec<(&str, State)> = SUPPORTED
        .iter()
        .map(|id| (*id, integration_state(id, home)))
        .collect();

    if json_output {
        let active = states.iter().filter(|(_, s)| *s == State::Active).count();
        let harnesses: Vec<_> = states
            .iter()
            .map(|(id, state)| {
                json!({
                    "id": id,
                    "name": harness_name(id),
                    "state": state_label(*state),
                })
            })
            .collect();
        let payload = json!({
            "active": active,
            "total": SUPPORTED.len(),
            "harnesses": harnesses,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?
        );
        return Ok(());
    }

    let opts = RenderOptions::stdout();
    println!(
        "{}",
        paint("┌  Topos Harness Status", Style::new().bold(), opts)
    );
    println!("│");

    let rank = |state: State| match state {
        State::Active => 0,
        State::Stale => 1,
        State::Absent => 2,
    };
    let mut sorted = states;
    sorted.sort_by_key(|(_, state)| rank(*state));

    let mut active_count = 0;
    for (idx, (id, state)) in sorted.iter().enumerate() {
        println!("│  {}", paint(harness_name(id), Style::new().bold(), opts));
        match state {
            State::Active => {
                println!("│    {} configured", paint("✓", Style::new().green(), opts));
                active_count += 1;
            }
            State::Stale => println!(
                "│    {} stale or incomplete — run `topos uninstall {id}` or `topos install {id}`",
                paint("▲", Style::new().color256(208), opts)
            ),
            State::Absent => println!(
                "│    {} not configured",
                paint("○", Style::new().dim(), opts)
            ),
        }
        if idx < sorted.len() - 1 {
            println!("│");
        }
    }

    println!("│");
    println!(
        "└  {}",
        paint(
            format!(
                "Done. {active_count}/{} harness integrations active.",
                SUPPORTED.len()
            ),
            Style::new().bold(),
            opts,
        )
    );
    Ok(())
}

fn state_label(state: State) -> &'static str {
    match state {
        State::Active => "active",
        State::Stale => "stale",
        State::Absent => "absent",
    }
}
