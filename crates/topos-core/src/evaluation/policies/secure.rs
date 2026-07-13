//! `Φ_SECURE`: policy translator for the SECURE generator.
//!
//! Maps CPG-based security observations into a [`ScoredDecision`].
//! `achieved` requires zero dangerous calls and zero taint flows;
//! `score` is `min(per-metric qualities)` for reporting only.
//!
//! Quality functions:
//! - `danger_quality = exp(-dangerous_calls / danger_scale)`
//! - `taint_quality  = exp(-taint_flows / taint_scale)`
//!
//! The SECURE badge is achieved if and only if there are zero dangerous
//! calls and zero taint flows (strict security). Gate comparisons and
//! interpretation prose live in [`super::gates`]; thresholds in
//! [`super::calibration`].

use std::collections::HashMap;

use super::base::ScoredDecision;
use super::calibration::SECURE;
use super::gates::evaluate_gates;

/// `Φ_SECURE` — score the SECURE generator from CPG observations.
pub fn score_secure(dangerous_calls: f64, taint_flows: f64) -> ScoredDecision {
    let metrics = HashMap::from([
        ("cpg.dangerous_calls".to_string(), dangerous_calls),
        ("cpg.taint_flows".to_string(), taint_flows),
    ]);

    let results = evaluate_gates(&metrics, Some("secure"), false);
    if results.is_empty() {
        // If no metrics are provided, we vacuously satisfy SECURE.
        return ScoredDecision {
            score: 1.0,
            achieved: true,
            interpretation: HashMap::new(),
        };
    }

    // Score shaping (reporting only): exponential decay stays local to Φ_SECURE.
    let qualities: Vec<f64> = results
        .iter()
        .map(|r| {
            let scale = if r.spec.metric == "cpg.dangerous_calls" {
                SECURE.danger_scale
            } else {
                SECURE.taint_scale
            };
            (-r.value.max(0.0) / scale).exp()
        })
        .collect();

    ScoredDecision {
        score: qualities.into_iter().fold(f64::INFINITY, f64::min),
        achieved: results.iter().all(|r| r.passed()),
        interpretation: results
            .iter()
            .map(|r| (r.spec.metric.to_string(), r.interpretation()))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn independent_thresholds_each_fail_alone() {
        assert!(score_secure(0.0, 0.0).achieved);
        assert!(!score_secure(1.0, 0.0).achieved);
        assert!(!score_secure(0.0, 1.0).achieved);
    }
}
