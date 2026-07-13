//! `Φ_SIMPLE`: policy translator for the SIMPLE generator.
//!
//! Maps CFG and AST observations into a [`ScoredDecision`]:
//!
//! ```text
//! Φ_SIMPLE(metrics) → ScoredDecision
//! achieved = (cyclomatic ≤ gate) ∧ (entropy in band) ∧ (max_func ≤ gate)
//! score    = min(per-metric qualities)   # reporting only; does not gate achieved
//! ```
//!
//! Gate comparisons and interpretation prose live in
//! [`super::gates`]; thresholds and normalization caps in
//! [`super::calibration`]. Only the score-shaping quality curves remain
//! local.

use std::collections::HashMap;

use super::base::ScoredDecision;
use super::calibration::SIMPLE;
use super::gates::{evaluate_gates, interpret_metric};

/// `Φ_SIMPLE` — score the SIMPLE generator using independent raw
/// thresholds.
///
/// `is_entrypoint_module`, when true, tolerates low entropy for
/// import/export-only entrypoint modules.
pub fn score_simple(
    cyclomatic: Option<f64>,
    entropy: Option<f64>,
    max_function_complexity: Option<f64>,
    is_entrypoint_module: bool,
) -> ScoredDecision {
    let mut metrics = HashMap::new();
    if let Some(v) = cyclomatic {
        metrics.insert("cfg.cyclomatic".to_string(), v);
    }
    if let Some(v) = entropy {
        metrics.insert("ast.entropy".to_string(), v);
    }
    if let Some(v) = max_function_complexity {
        metrics.insert("ast.max_function_complexity".to_string(), v);
    }

    let results = evaluate_gates(&metrics, Some("simple"), is_entrypoint_module);
    if results.is_empty() {
        // If no metrics are provided, we vacuously satisfy SIMPLE.
        return ScoredDecision {
            score: 1.0,
            achieved: true,
            interpretation: HashMap::new(),
        };
    }

    // Score shaping (reporting only): quality curves stay local to Φ_SIMPLE.
    let qualities: Vec<f64> = results
        .iter()
        .map(|r| quality(r.spec.metric, r.value))
        .collect();

    ScoredDecision {
        // The combined score is the minimum of the individual qualities
        // (conservative AND).
        score: qualities.into_iter().fold(f64::INFINITY, f64::min),
        achieved: results.iter().all(|r| r.passed()),
        interpretation: results
            .iter()
            .map(|r| (r.spec.metric.to_string(), r.interpretation()))
            .collect(),
    }
}

/// Normalize one raw metric to a `[0, 1]` quality (never gates `achieved`).
fn quality(metric: &str, value: f64) -> f64 {
    match metric {
        "cfg.cyclomatic" => 1.0 - (value / SIMPLE.max_cyclomatic_cap).min(1.0),
        "ast.entropy" => (1.0 - 2.0 * (value - SIMPLE.entropy_ideal).abs()).max(0.0),
        _ => 1.0 - (value / SIMPLE.max_function_complexity_cap).min(1.0),
    }
}

/// Describe a raw AST entropy ratio using SIMPLE policy language.
pub fn describe_entropy_ratio(entropy: f64) -> String {
    interpret_metric("ast.entropy", entropy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfect_code_scores_one() {
        // Ideal: cyclomatic=0, entropy=0.5, max_func=0 -> score is 1.0
        let result = score_simple(Some(0.0), Some(0.5), Some(0.0), false);
        assert_eq!(result.score, 1.0);
        assert!(result.achieved);
    }

    #[test]
    fn pathological_code_scores_zero() {
        // Worst case: cyclomatic=40, entropy=1.0, max_func=20 -> score is 0.0
        let result = score_simple(Some(40.0), Some(1.0), Some(20.0), false);
        assert_eq!(result.score, 0.0);
        assert!(!result.achieved);
    }

    #[test]
    fn independent_thresholds_each_fail_alone() {
        assert!(score_simple(Some(10.0), Some(0.5), Some(5.0), false).achieved);
        assert!(!score_simple(Some(16.0), Some(0.5), Some(5.0), false).achieved); // fail cyclomatic
        assert!(!score_simple(Some(10.0), Some(0.9), Some(5.0), false).achieved); // fail entropy
        assert!(!score_simple(Some(10.0), Some(0.5), Some(11.0), false).achieved);
        // fail max func
    }

    #[test]
    fn no_metrics_vacuously_satisfies() {
        let result = score_simple(None, None, None, false);
        assert!(result.achieved);
        assert_eq!(result.score, 1.0);
    }
}
