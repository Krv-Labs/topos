//! `Φ_COMPOSABLE`: policy translator for the COMPOSABLE generator.
//!
//! Maps `ModuleDependencyGraph` metric observations (Martin instability,
//! fan-in, fan-out) into a [`ScoredDecision`]. `achieved` is the AND of
//! independent raw thresholds on each metric; `score` is
//! `min(per-metric qualities)` for reporting only.
//!
//! Quality functions:
//! - `instability_quality` — a flat-top tent over `[low, high]`: in-band
//!   → `1.0`; below `low` → linear from `0.0` to `1.0`; above `high` →
//!   linear from `1.0` to `0.0`.
//! - `fan_quality = 1 - min(fan / cap, 1.0)` — linear fall from `1.0` to
//!   `0.0` at the cap.

use std::collections::HashMap;

use super::base::ScoredDecision;
use super::calibration::COMPOSABLE;
use super::gates::evaluate_gates;

/// `Φ_COMPOSABLE` — score the COMPOSABLE generator using independent
/// raw thresholds.
///
/// `is_entrypoint_module`, when true, tolerates high instability for
/// import/export-only entrypoint modules with zero fan-in.
pub fn score_coupling(
    instability: Option<f64>,
    fan_in: Option<f64>,
    fan_out: Option<f64>,
    is_entrypoint_module: bool,
) -> ScoredDecision {
    let mut metrics = HashMap::new();
    if let Some(v) = instability {
        metrics.insert("mdg.instability".to_string(), v);
    }
    if let Some(v) = fan_in {
        metrics.insert("mdg.fan_in".to_string(), v);
    }
    if let Some(v) = fan_out {
        metrics.insert("mdg.fan_out".to_string(), v);
    }

    let results = evaluate_gates(&metrics, Some("composable"), is_entrypoint_module);
    if results.is_empty() {
        // If no metrics are provided, we vacuously satisfy COMPOSABLE.
        return ScoredDecision {
            score: 1.0,
            achieved: true,
            interpretation: HashMap::new(),
        };
    }

    let qualities: Vec<f64> = results
        .iter()
        .map(|r| quality(r.spec.metric, r.value))
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

fn quality(metric: &str, value: f64) -> f64 {
    if metric == "mdg.instability" {
        return instability_tent(value);
    }
    let cap = if metric == "mdg.fan_in" {
        COMPOSABLE.max_fan_in_cap
    } else {
        COMPOSABLE.max_fan_out_cap
    };
    1.0 - (value / cap).min(1.0)
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
        let result = score_coupling(Some(0.5), Some(5.0), Some(5.0), false);
        assert!(result.achieved);
        assert_eq!(result.score, 0.875);
    }

    #[test]
    fn zero_fan_and_ideal_instability_scores_one() {
        let result = score_coupling(Some(0.5), Some(0.0), Some(0.0), false);
        assert!(result.achieved);
        assert_eq!(result.score, 1.0);
    }

    #[test]
    fn excessive_fan_out_fails() {
        let result = score_coupling(Some(0.5), Some(5.0), Some(30.0), false);
        assert!(!result.achieved);
    }

    #[test]
    fn no_metrics_vacuously_satisfies() {
        assert!(score_coupling(None, None, None, false).achieved);
    }
}
