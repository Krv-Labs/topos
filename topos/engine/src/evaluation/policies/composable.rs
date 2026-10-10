//! `Φ_COMPOSABLE`: policy translator for the COMPOSABLE generator.
//!
//! At file scope only outward burden gates: `achieved = (fan_out ≤ gate)`
//! and the score is its gate-anchored desirability (`d = TAU` at the gate,
//! see [`super::gates::Curve::LowerIsBetter`]).
//!
//! Martin instability and fan-in stay raw metrics but are advisory: their
//! readings are judged relative to the codebase by
//! [`crate::evaluation::advisory`], never against a fixed band. A fixed
//! instability band at file granularity tracked the ratio's `1 / (Ca + Ce)`
//! resolution grid rather than design quality, and the main-sequence
//! distance built on it inherited the same limit (issue #351; see
//! `docs/decisions/composable-at-module-granularity.md`). Incoming fan-in
//! measures change-impact radius, not dependency burden: a stable interface
//! can legitimately have many callers (Basili, Briand & Melo 1996;
//! Zimmermann & Nagappan 2008).

use std::collections::BTreeMap;

use super::base::ScoredDecision;
use super::gates::evaluate_gates;

/// `Φ_COMPOSABLE` — score the COMPOSABLE generator from file-level
/// fan-out (distinct external callees).
pub fn score_coupling(fan_out: Option<f64>) -> ScoredDecision {
    let mut metrics = BTreeMap::new();
    if let Some(v) = fan_out {
        metrics.insert("mdg.fan_out".to_string(), v);
    }
    ScoredDecision::from_gates(&evaluate_gates(&metrics, Some("composable"), false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::policies::desirability::TAU;

    #[test]
    fn balanced_module_achieves_composable() {
        // fan_out 5 of gate 10 -> d = 1 − 0.5·0.5 = 0.75.
        let result = score_coupling(Some(5.0));
        assert!(result.achieved);
        assert_eq!(result.score, 0.75);
    }

    #[test]
    fn zero_fan_out_scores_one() {
        let result = score_coupling(Some(0.0));
        assert!(result.achieved);
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn excessive_fan_out_fails() {
        let result = score_coupling(Some(30.0));
        assert!(!result.achieved);
        assert_eq!(result.score, 0.0);
    }

    #[test]
    fn no_metrics_vacuously_satisfies() {
        let result = score_coupling(None);
        assert!(result.achieved);
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn achieved_iff_score_clears_tau() {
        for fan_out in [0.0, 3.0, 9.0, 10.0, 11.0, 19.0, 20.0, 40.0] {
            let r = score_coupling(Some(fan_out));
            assert_eq!(r.achieved, r.gate_score >= TAU);
            assert_eq!(r.achieved, r.score >= TAU);
        }
    }
}
