//! `Φ_NAVIGABLE`: policy translator for the NAVIGABLE generator.
//!
//! Maps the AST divergence observation into a [`ScoredDecision`]:
//!
//! ```text
//! Φ_NAVIGABLE(metrics) → ScoredDecision
//! achieved = (max_function_divergence ≤ gate)
//! score    = 1 - min(divergence / cap, 1)   # reporting only
//! ```
//!
//! One gating metric, deliberately. NAVIGABLE answers a single question —
//! how deeply nested is the worst function an agent has to hold in its
//! head — and the sub-metrics from the LM-CC literature that would join
//! it (neighborhood entropy, scope attention density) either need the
//! GitNexus dependency graph or re-measure what `ast.entropy` already
//! covers under SIMPLE. See `functors::probes::ast::divergence`.
//!
//! Gate comparisons and interpretation prose live in [`super::gates`];
//! thresholds and normalization caps in [`super::calibration`]. Only the
//! score-shaping quality curve remains local.

use std::collections::HashMap;

use super::base::ScoredDecision;
use super::calibration::NAVIGABLE;
use super::gates::{evaluate_gates, interpret_metric};

/// `Φ_NAVIGABLE` — score the NAVIGABLE generator from the worst
/// function's Semantic Compositional Divergence.
pub fn score_navigable(max_function_divergence: Option<f64>) -> ScoredDecision {
    let mut metrics = HashMap::new();
    if let Some(v) = max_function_divergence {
        metrics.insert("nav.max_function_divergence".to_string(), v);
    }

    let results = evaluate_gates(&metrics, Some("navigable"), false, false, None);
    if results.is_empty() {
        // No metrics provided (unparseable input) — vacuously satisfied,
        // matching `Φ_SIMPLE`.
        return ScoredDecision {
            score: 1.0,
            achieved: true,
            interpretation: HashMap::new(),
        };
    }

    ScoredDecision {
        score: results
            .iter()
            .map(|r| quality(r.value))
            .fold(f64::INFINITY, f64::min),
        achieved: results
            .iter()
            .filter(|r| r.spec.gates_achieved)
            .all(|r| r.passed()),
        interpretation: results
            .iter()
            .map(|r| (r.spec.metric.to_string(), r.interpretation()))
            .collect(),
    }
}

/// Normalize divergence to a `[0, 1]` quality (never gates `achieved`).
///
/// Linear decay to the cap: unlike entropy there is no "too little
/// nesting" failure mode, so the curve is one-sided.
fn quality(divergence: f64) -> f64 {
    1.0 - (divergence / NAVIGABLE.divergence_cap).min(1.0)
}

/// Describe a raw divergence reading using NAVIGABLE policy language.
pub fn describe_divergence(divergence: f64) -> String {
    interpret_metric("nav.max_function_divergence", divergence)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_code_achieves_navigable_with_a_perfect_score() {
        let decision = score_navigable(Some(0.0));
        assert!(decision.achieved);
        assert_eq!(decision.score, 1.0);
    }

    #[test]
    fn divergence_over_the_gate_fails() {
        let decision = score_navigable(Some(NAVIGABLE.max_function_divergence + 1.0));
        assert!(!decision.achieved);
        assert!(decision.score < 1.0);
    }

    #[test]
    fn divergence_exactly_at_the_gate_passes() {
        let decision = score_navigable(Some(NAVIGABLE.max_function_divergence));
        assert!(decision.achieved);
    }

    #[test]
    fn missing_metric_is_a_vacuous_pass() {
        let decision = score_navigable(None);
        assert!(decision.achieved);
        assert_eq!(decision.score, 1.0);
        assert!(decision.interpretation.is_empty());
    }

    #[test]
    fn score_decays_monotonically_and_floors_at_the_cap() {
        let mid = score_navigable(Some(NAVIGABLE.divergence_cap / 2.0)).score;
        let worse = score_navigable(Some(NAVIGABLE.divergence_cap)).score;
        let beyond = score_navigable(Some(NAVIGABLE.divergence_cap * 10.0)).score;
        assert!(mid > worse);
        assert_eq!(worse, 0.0);
        assert_eq!(beyond, 0.0, "score must floor, never go negative");
    }

    #[test]
    fn interpretation_names_the_metric_and_the_threshold() {
        let decision = score_navigable(Some(NAVIGABLE.max_function_divergence + 5.0));
        let text = &decision.interpretation["nav.max_function_divergence"];
        assert!(text.contains("exceeds threshold"), "{text}");
    }
}
