//! `Φ_COMPOSABLE`: policy translator for the COMPOSABLE generator.
//!
//! Maps `ModuleDependencyGraph` metric observations (Martin instability,
//! fan-in, fan-out) into a [`ScoredDecision`]. At file scope, only the raw
//! fan-out threshold gates `achieved`; the other readings remain scored and
//! actionable diagnostics. `score` is `min(per-metric qualities)` for
//! reporting only.
//!
//! Quality functions:
//! - `instability_quality` — a flat-top tent over `[low, high]`: in-band
//!   → `1.0`; below `low` → linear from `0.0` to `1.0`; above `high` →
//!   linear from `1.0` to `0.0`.
//! - `fan_quality = 1 - min(fan / cap, 1.0)` — linear fall from `1.0` to
//!   `0.0` at the cap.

use std::collections::BTreeMap;

use super::base::ScoredDecision;
use super::calibration::COMPOSABLE;
use super::gates::evaluate_gates;
use crate::functors::probes::mdg::coupling::MIN_RESOLVABLE_COUPLING;

/// `Φ_COMPOSABLE` — score the COMPOSABLE generator using independent
/// raw thresholds.
///
/// `is_entrypoint_module`, when true, tolerates high instability for
/// import/export-only entrypoint modules with zero fan-in.
pub fn score_coupling(
    instability: Option<f64>,
    fan_in: Option<f64>,
    fan_out: Option<f64>,
    abstractness: Option<f64>,
    coupling: Option<f64>,
    is_entrypoint_module: bool,
    is_stable_leaf_module: bool,
) -> ScoredDecision {
    let metrics = coupling_gate_input(instability, fan_in, fan_out, abstractness, coupling);
    // Distance mode is active iff the shared gate-input builder chose it
    // (abstractness + a real coupling signal present); see
    // `coupling_gate_input`.
    let use_distance = metrics.contains_key("mdg.main_sequence_distance");

    let results = evaluate_gates(
        &metrics,
        Some("composable"),
        is_entrypoint_module,
        is_stable_leaf_module,
        instability,
    );
    if results.is_empty() {
        // If no metrics are provided, we vacuously satisfy COMPOSABLE.
        return ScoredDecision {
            score: 1.0,
            achieved: true,
            interpretation: BTreeMap::new(),
        };
    }

    let qualities: Vec<f64> = results
        .iter()
        .map(|r| quality(r.spec.metric, r.value))
        .collect();

    let mut interpretation: BTreeMap<String, String> = results
        .iter()
        .map(|r| (r.spec.metric.to_string(), r.interpretation()))
        .collect();
    if use_distance {
        // `mdg.instability` is deliberately not gated when distance is
        // active, but users should still see why a high/low instability
        // reading isn't itself a failure -- surface it as an informational
        // (non-gating) line alongside the distance verdict.
        interpretation.insert(
            "mdg.instability".to_string(),
            crate::evaluation::policies::gates::interpret_metric(
                "mdg.instability",
                instability.unwrap(),
            ),
        );
    }

    ScoredDecision {
        score: qualities.into_iter().fold(f64::INFINITY, f64::min),
        achieved: results
            .iter()
            .filter(|r| r.spec.gates_achieved)
            .all(|r| r.passed()),
        interpretation,
    }
}

/// The exact metric map `Φ_COMPOSABLE` scores and evaluates.
///
/// Instability is replaced by `mdg.main_sequence_distance = |A + I − 1|`
/// whenever abstractness *and* a real coupling signal are present. A file
/// whose coupling cannot resolve an instability ratio (`calculate_coupling`'s
/// 0.5 "no signal" fallback) keeps the raw instability diagnostic rather than
/// fabricating a distance from that fallback. Shared with the suggestion engine
/// so a suggestion can never fire on a metric the scorer did not use.
///
/// `coupling` is import-graph `Ca + Ce` (`mdg.coupling`), which is where the
/// instability ratio comes from. This deliberately does *not* read
/// `mdg.fan_in`/`mdg.fan_out`: those count symbol-level `CALLS` edges, a
/// different graph. Testing them here failed every module whose members make
/// no calls — a pure trait/interface module reads zero fan while carrying a
/// dozen real `IMPORTS` edges, so `topos/engine/src/graphs/base.rs` sat exactly
/// on the main sequence (`A = 1.0`, `I = 0.0`, `D = 0.0`) and still scored
/// COMPOSABLE at 0.0.
pub fn coupling_gate_input(
    instability: Option<f64>,
    fan_in: Option<f64>,
    fan_out: Option<f64>,
    abstractness: Option<f64>,
    coupling: Option<f64>,
) -> BTreeMap<String, f64> {
    let has_coupling_signal = coupling.is_some_and(|c| c >= MIN_RESOLVABLE_COUPLING as f64);
    // `mdg.abstractness = 0.0` is both "genuinely concrete" and "nothing
    // abstract was detected", and for languages where Topos finds no abstract
    // types it is 0.0 across an entire repo. Pinning A at 0 collapses
    // `|A + I − 1|` into `1 − I`, which rewards maximally-unstable modules
    // and scores every stable one at 0 — the inverse of what the main
    // sequence means. Require abstractness to carry signal, the same way
    // `has_coupling_signal` requires it of fan.
    let has_abstractness_signal = abstractness.is_some_and(|a| a > 0.0);
    let use_distance = instability.is_some() && has_abstractness_signal && has_coupling_signal;

    let mut metrics = BTreeMap::new();
    if use_distance {
        let distance = (abstractness.unwrap() + instability.unwrap() - 1.0).abs();
        metrics.insert("mdg.main_sequence_distance".to_string(), distance);
    } else if let Some(v) = instability {
        metrics.insert("mdg.instability".to_string(), v);
    }
    if let Some(v) = fan_in {
        metrics.insert("mdg.fan_in".to_string(), v);
    }
    if let Some(v) = fan_out {
        metrics.insert("mdg.fan_out".to_string(), v);
    }
    metrics
}

fn quality(metric: &str, value: f64) -> f64 {
    match metric {
        "mdg.instability" => instability_tent(value),
        "mdg.main_sequence_distance" => distance_quality(value),
        "mdg.fan_in" => 1.0 - (value / COMPOSABLE.max_fan_in_cap).min(1.0),
        _ => 1.0 - (value / COMPOSABLE.max_fan_out_cap).min(1.0),
    }
}

/// Linear fall from `1.0` (on the main sequence) to `0.0` at the cap.
fn distance_quality(distance: f64) -> f64 {
    1.0 - (distance / COMPOSABLE.main_sequence_distance_max).min(1.0)
}

/// Flat-top tent function over `[instability_low, instability_high]`.
fn instability_tent(instability: f64) -> f64 {
    let (low, high) = (COMPOSABLE.instability_low, COMPOSABLE.instability_high);
    if (low..=high).contains(&instability) {
        1.0
    } else if instability < low {
        instability / low
    } else {
        ((1.0 - instability) / (1.0 - high)).max(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_module_achieves_composable() {
        // instability=0.5 is in-band -> quality 1.0; fan_in/fan_out=5.0
        // are nonzero -> quality 1 - 5/40 = 0.875 each, so the combined
        // (min) score is 0.875, not 1.0 — only all-zero fan would give 1.0.
        let result = score_coupling(
            Some(0.5),
            Some(5.0),
            Some(5.0),
            None,
            Some(4.0),
            false,
            false,
        );
        assert!(result.achieved);
        assert_eq!(result.score, 0.875);
    }

    #[test]
    fn zero_fan_and_ideal_instability_scores_one() {
        let result = score_coupling(
            Some(0.5),
            Some(0.0),
            Some(0.0),
            None,
            Some(4.0),
            false,
            false,
        );
        assert!(result.achieved);
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn excessive_fan_out_fails() {
        let result = score_coupling(
            Some(0.5),
            Some(5.0),
            Some(30.0),
            None,
            Some(4.0),
            false,
            false,
        );
        assert!(!result.achieved);
    }

    #[test]
    fn excessive_fan_in_is_advisory_at_file_scope() {
        let result = score_coupling(
            Some(0.5),
            Some(30.0),
            Some(5.0),
            None,
            Some(4.0),
            false,
            false,
        );
        assert!(result.achieved);
        assert_eq!(result.score, 0.25);
    }

    #[test]
    fn no_metrics_vacuously_satisfies() {
        assert!(score_coupling(None, None, None, None, None, false, false).achieved);
    }

    /// `mdg.instability` is advisory (`gates_achieved: false`): a reading
    /// far outside the band still drags the reported score down, but it
    /// cannot cost the file its COMPOSABLE verdict on its own.
    #[test]
    fn out_of_band_instability_scores_low_but_still_achieves() {
        let result = score_coupling(
            Some(1.0),
            Some(2.0),
            Some(3.0),
            None,
            Some(4.0),
            false,
            false,
        );
        assert!(
            result.achieved,
            "instability alone must not fail COMPOSABLE"
        );
        assert_eq!(result.score, 0.0, "but it still shows in the score");
    }

    /// The `graphs/base.rs` case: a pure trait module makes no calls, so
    /// its symbol-level fan is legitimately zero while it carries a dozen
    /// real `IMPORTS` edges. Reading fan as the coupling signal suppressed
    /// distance mode and scored a module sitting exactly on the main
    /// sequence (`A = 1.0`, `I = 0.0`) at 0.0.
    #[test]
    fn abstraction_module_with_no_calls_uses_distance() {
        let metrics = coupling_gate_input(Some(0.0), Some(0.0), Some(0.0), Some(1.0), Some(12.0));
        assert_eq!(metrics.get("mdg.main_sequence_distance"), Some(&0.0));
        assert!(!metrics.contains_key("mdg.instability"));

        let result = score_coupling(
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(1.0),
            Some(12.0),
            false,
            false,
        );
        assert!(result.achieved);
        assert_eq!(result.score, 1.0);
    }

    /// Coupling below the ratio's resolution limit cannot build a distance:
    /// `I` is the 0.5 no-signal fallback there, so `|A + I − 1|` would be
    /// `|A − 0.5|` — for a fully abstract module, exactly the max.
    #[test]
    fn unresolvable_coupling_keeps_raw_instability_diagnostic() {
        let metrics = coupling_gate_input(Some(0.5), Some(1.0), Some(1.0), Some(1.0), Some(1.0));
        assert!(!metrics.contains_key("mdg.main_sequence_distance"));
        assert_eq!(metrics.get("mdg.instability"), Some(&0.5));
    }

    /// Distance needs a real abstractness reading. With `A = 0.0`,
    /// `|A + I − 1|` is just `1 − I`, so a maximally stable module (`I = 0`)
    /// would land on the worst possible distance for being depended upon.
    #[test]
    fn zero_abstractness_keeps_raw_instability_diagnostic() {
        let metrics = coupling_gate_input(Some(0.0), Some(2.0), Some(8.0), Some(0.0), Some(6.0));
        assert!(!metrics.contains_key("mdg.main_sequence_distance"));
        assert_eq!(metrics.get("mdg.instability"), Some(&0.0));

        let real = coupling_gate_input(Some(0.0), Some(2.0), Some(8.0), Some(0.4), Some(6.0));
        assert_eq!(real.get("mdg.main_sequence_distance"), Some(&0.6));
        assert!(!real.contains_key("mdg.instability"));
    }
}
