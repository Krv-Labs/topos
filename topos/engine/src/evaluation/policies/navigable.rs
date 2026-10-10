//! `Φ_NAVIGABLE`: policy translator for the NAVIGABLE generator.
//!
//! Maps the AST divergence observation into a [`ScoredDecision`]:
//!
//! ```text
//! Φ_NAVIGABLE(metrics) → ScoredDecision
//! achieved = (max_function_divergence ≤ gate)
//! G        = d(divergence)   # lower-is-better: 1 at 0, TAU at gate, 0 at 2·gate
//! ```
//!
//! One gating metric, deliberately. NAVIGABLE answers a single question —
//! how deeply nested is the worst function an agent has to hold in its
//! head — and the sub-metrics from the LM-CC literature that would join
//! it (neighborhood entropy, scope attention density) either need the
//! GitNexus dependency graph or re-measure what `ast.entropy` already
//! covers under SIMPLE. See `functors::probes::ast::divergence`.
//!
//! Gate comparisons, the desirability curve, and interpretation prose
//! live in [`super::gates`]; the threshold in [`super::calibration`].

use std::collections::BTreeMap;

use super::base::ScoredDecision;
use super::gates::{evaluate_gates, interpret_metric};

/// `Φ_NAVIGABLE` — score the NAVIGABLE generator from the worst
/// function's Semantic Compositional Divergence.
pub fn score_navigable(max_function_divergence: Option<f64>) -> ScoredDecision {
    let mut metrics = BTreeMap::new();
    if let Some(v) = max_function_divergence {
        metrics.insert("nav.max_function_divergence".to_string(), v);
    }

    ScoredDecision::from_gates(&evaluate_gates(&metrics, Some("navigable"), false))
}

/// Describe a raw divergence reading using NAVIGABLE policy language.
pub fn describe_divergence(divergence: f64) -> String {
    interpret_metric("nav.max_function_divergence", divergence)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::policies::calibration::NAVIGABLE;
    use crate::evaluation::policies::desirability::TAU;

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
    fn score_decays_monotonically_and_floors_at_twice_the_gate() {
        let gate = NAVIGABLE.max_function_divergence;
        let mid = score_navigable(Some(gate / 2.0)).score;
        let at_gate = score_navigable(Some(gate)).score;
        let floor = score_navigable(Some(2.0 * gate)).score;
        let beyond = score_navigable(Some(gate * 10.0)).score;
        assert!(mid > at_gate);
        assert_eq!(at_gate, TAU);
        assert_eq!(floor, 0.0);
        assert_eq!(beyond, 0.0, "score must floor, never go negative");
    }

    #[test]
    fn achieved_iff_score_clears_tau() {
        for divergence in [0.0, 4.0, 9.99, 10.0, 10.01, 15.0, 20.0, 99.0] {
            let r = score_navigable(Some(divergence));
            assert_eq!(r.achieved, r.gate_score >= TAU);
            assert_eq!(r.achieved, r.score >= TAU);
        }
    }

    #[test]
    fn interpretation_names_the_metric_and_the_threshold() {
        let decision = score_navigable(Some(NAVIGABLE.max_function_divergence + 5.0));
        let text = &decision.interpretation["nav.max_function_divergence"];
        assert!(text.contains("exceeds threshold"), "{text}");
    }
}
