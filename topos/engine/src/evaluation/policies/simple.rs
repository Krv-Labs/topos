//! `Φ_SIMPLE`: policy translator for the SIMPLE generator.
//!
//! Maps AST observations into a [`ScoredDecision`]:
//!
//! ```text
//! Φ_SIMPLE(metrics) → ScoredDecision
//! achieved = (entropy in band) ∧ (max_func ≤ gate)
//! G        = min(d_entropy, d_max_func)   # gate-anchored: G ≥ TAU ⇔ achieved
//! ```
//!
//! `cfg.cyclomatic` is advisory (issue #193): it's a whole-file
//! merged-CFG sum that scales with function count, so it would otherwise
//! hard-fail a file with several small, individually-simple functions --
//! a concern `max_func` (a true per-function max) already gates directly.
//! It no longer enters this score at all; it is read relative to its
//! codebase by [`crate::evaluation::advisory`].
//!
//! Gate comparisons, desirability curves, and interpretation prose live in
//! [`super::gates`]; thresholds in [`super::calibration`]. Only the
//! tiny-file entropy floor (issue #152) remains local.

use std::collections::BTreeMap;

use super::base::ScoredDecision;
use super::calibration::SIMPLE;
use super::gates::{evaluate_gates, interpret_metric, GateResult};

/// `Φ_SIMPLE` — score the SIMPLE generator using independent raw
/// thresholds.
///
/// `is_entrypoint_module`, when true, tolerates out-of-band entropy for
/// import/export-only entrypoint modules.
pub fn score_simple(
    entropy: Option<f64>,
    max_function_complexity: Option<f64>,
    is_entrypoint_module: bool,
    source_size_bytes: Option<f64>,
) -> ScoredDecision {
    let mut metrics = BTreeMap::new();
    if let Some(v) = entropy {
        metrics.insert("ast.entropy".to_string(), v);
    }
    if let Some(v) = max_function_complexity {
        metrics.insert("ast.max_function_complexity".to_string(), v);
    }

    let results = evaluate_gates(&metrics, Some("simple"), is_entrypoint_module);
    ScoredDecision::from_gates_with(&results, |r| desirability(r, source_size_bytes))
}

/// Gate-anchored desirability, with the tiny-file entropy floor.
fn desirability(r: &GateResult, source_size_bytes: Option<f64>) -> f64 {
    let below_floor =
        source_size_bytes.is_some_and(|bytes| bytes < SIMPLE.entropy_size_floor_bytes);
    if r.spec.metric == "ast.entropy" && below_floor && r.value > SIMPLE.entropy_ideal && r.passed()
    {
        // Tiny inputs inflate the ratio via zlib's fixed per-stream
        // overhead (issue #152): an above-ideal reading isn't a reliable
        // "too dense" signal at this size, so it doesn't sink the score.
        // Only on the pass side -- a failing gate must stay below TAU.
        // Below-ideal (repetition) stays fully penalized -- that signal
        // holds at any size.
        1.0
    } else {
        r.desirability()
    }
}

/// Describe a raw AST entropy ratio using SIMPLE policy language.
pub fn describe_entropy_ratio(entropy: f64) -> String {
    interpret_metric("ast.entropy", entropy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::policies::desirability::TAU;

    #[test]
    fn perfect_code_scores_one() {
        // Ideal: entropy=0.5, max_func=0 -> score is 1.0
        let result = score_simple(Some(0.5), Some(0.0), false, None);
        assert_eq!(result.score, 1.0);
        assert_eq!(result.gate_score, 1.0);
        assert!(result.achieved);
    }

    #[test]
    fn pathological_code_scores_zero() {
        // Worst case: entropy=1.0, max_func=20 (= 2 × gate) -> score is 0.0
        let result = score_simple(Some(1.0), Some(20.0), false, None);
        assert!(result.score.abs() < 1e-12);
        assert!(!result.achieved);
    }

    #[test]
    fn independent_thresholds_each_fail_alone() {
        assert!(score_simple(Some(0.5), Some(5.0), false, None).achieved);
        assert!(!score_simple(Some(0.9), Some(5.0), false, None).achieved); // fail entropy
        assert!(!score_simple(Some(0.5), Some(11.0), false, None).achieved);
        // fail max func
    }

    #[test]
    fn score_is_the_gate_anchored_minimum() {
        // max_func 8 of gate 10 -> d = 1 − 0.5·0.8 = 0.6; entropy ideal -> 1.
        let result = score_simple(Some(0.5), Some(8.0), false, None);
        assert!((result.score - 0.6).abs() < 1e-12);
        // At the gate exactly: d = TAU, still passing.
        let at_gate = score_simple(Some(0.5), Some(10.0), false, None);
        assert!(at_gate.achieved);
        assert_eq!(at_gate.score, TAU);
    }

    #[test]
    fn achieved_iff_score_clears_tau() {
        for entropy in [0.0, 0.1, 0.2, 0.35, 0.5, 0.65, 0.8, 0.9, 1.0] {
            for max_func in [0.0, 5.0, 10.0, 10.5, 15.0, 25.0] {
                for entrypoint in [false, true] {
                    for size in [None, Some(50.0), Some(5000.0)] {
                        let r = score_simple(Some(entropy), Some(max_func), entrypoint, size);
                        assert_eq!(r.achieved, r.gate_score >= TAU);
                        assert_eq!(r.achieved, r.score >= TAU);
                    }
                }
            }
        }
    }

    #[test]
    fn tiny_file_floor_only_lifts_passing_entropy() {
        // Above ideal but in band: lifted to 1.0 on a tiny file.
        assert_eq!(score_simple(Some(0.7), None, false, Some(50.0)).score, 1.0);
        // Out of band: the gate fails, so the floor must not lift it.
        let failing = score_simple(Some(0.9), None, false, Some(50.0));
        assert!(!failing.achieved);
        assert!(failing.score < TAU);
    }

    #[test]
    fn exempt_entrypoint_entropy_scores_on_the_passing_side() {
        let result = score_simple(Some(0.05), Some(0.0), true, None);
        assert!(result.achieved);
        assert_eq!(result.score, TAU);
    }

    #[test]
    fn no_metrics_vacuously_satisfies() {
        let result = score_simple(None, None, false, None);
        assert!(result.achieved);
        assert_eq!(result.score, 1.0);
    }
}
