//! `Φ_SECURE`: policy translator for the SECURE generator.
//!
//! Maps CPG-based security observations into a [`ScoredDecision`].
//! `achieved` requires zero dangerous calls and zero taint flows; the
//! score is `G = min(d_danger, d_taint)` over zero-tolerance
//! desirabilities:
//!
//! ```text
//! d(0) = 1,   d(v > 0) = TAU·exp(−v / scale)
//! ```
//!
//! so any finding lands strictly below `TAU` (strict security). Gate
//! comparisons, curves, and interpretation prose live in [`super::gates`];
//! thresholds and decay scales in [`super::calibration`].

use std::collections::BTreeMap;

use super::base::ScoredDecision;
use super::gates::evaluate_gates;

/// `Φ_SECURE` — score the SECURE generator from CPG observations.
pub fn score_secure(dangerous_calls: f64, taint_flows: f64) -> ScoredDecision {
    let metrics = BTreeMap::from([
        ("cpg.dangerous_calls".to_string(), dangerous_calls),
        ("cpg.taint_flows".to_string(), taint_flows),
    ]);

    ScoredDecision::from_gates(&evaluate_gates(&metrics, Some("secure"), false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evaluation::policies::desirability::TAU;

    #[test]
    fn clean_code_scores_one() {
        let result = score_secure(0.0, 0.0);
        assert_eq!(result.score, 1.0);
        assert!(result.achieved);
    }

    #[test]
    fn dangerous_code_scores_low_and_fails() {
        let result = score_secure(20.0, 20.0);
        assert!(result.score < 0.1);
        assert!(!result.achieved);
    }

    #[test]
    fn one_finding_drops_just_below_tau() {
        let result = score_secure(1.0, 0.0);
        assert!(result.score < TAU);
        assert!(result.score > 0.3);
    }

    #[test]
    fn achieved_iff_score_clears_tau() {
        for dangerous in [0.0, 1.0, 3.0, 50.0] {
            for taint in [0.0, 1.0, 5.0] {
                let r = score_secure(dangerous, taint);
                assert_eq!(r.achieved, r.gate_score >= TAU);
                assert_eq!(r.achieved, r.score >= TAU);
            }
        }
    }

    #[test]
    fn independent_thresholds_each_fail_alone() {
        assert!(score_secure(0.0, 0.0).achieved);
        assert!(!score_secure(1.0, 0.0).achieved);
        assert!(!score_secure(0.0, 1.0).achieved);
    }
}
