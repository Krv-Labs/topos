//! Shared types for the policy translators `Φᵢ : ℝ → Ω`.
//!
//! Following the math spec (§3 "Policy Translation"), each quality
//! generator `gᵢ ∈ G_qual` has an associated policy translator `Φᵢ`
//! that maps probe outputs into a [`ScoredDecision`]. The characteristic
//! morphism ([`crate::evaluation::characteristic_morphism`]) reads each
//! decision's `achieved` flag and assembles the 8-element verdict in
//! `Ω` via [`crate::core::omega::verdict_from_generators`].
//!
//! There is exactly one `Φᵢ` per generator:
//! - `Φ_SIMPLE` ↦ `policies::simple::score_simple`
//! - `Φ_COMPOSABLE` ↦ `policies::composable::score_coupling`
//! - `Φ_SECURE` ↦ `policies::secure::score_secure`
//!
//! # Decisive semantics: AND-of-raw-metric thresholds
//!
//! Each `Φᵢ` owns **per-metric raw thresholds** (cyclomatic ≤ 15, zero
//! taint flows, fan-in ≤ 15, …). `achieved` is the independent AND of
//! those checks — *not* `score ≥ score_floor(g)`. The normalized
//! `score` on [`ScoredDecision`] is `min(per-metric qualities)` for
//! reporting and multi-file meets; it does not gate `achieved`.
//!
//! [`meet_satisfied`] implements an *alternate* score-floor gate
//! (`score ≥ score_floor(g)`) for callers that already hold normalized
//! scores. The live `CharacteristicMorphism` path does **not** use it —
//! it trusts `ScoredDecision.achieved` from each `Φᵢ`.

use std::collections::HashMap;

use crate::evaluation::policies::calibration::score_floor;
use crate::evaluation::preferences::Generator;

/// Normalized score floor for one generator (score-floor path only).
pub fn threshold(generator: Generator) -> f64 {
    score_floor(generator)
}

/// Whether a normalized score clears the score-floor for one generator.
pub fn is_satisfied(generator: Generator, score: f64) -> bool {
    score >= threshold(generator)
}

/// Score-floor AND across generators, for pre-aggregated normalized
/// scores. Feed into [`crate::core::omega::verdict_from_generators`] for
/// the `Ω` element.
///
/// Prefer each `Φᵢ`'s `ScoredDecision.achieved` when probe metrics are
/// available — that path applies raw-metric gates from
/// [`crate::evaluation::policies::calibration`].
pub fn meet_satisfied(scores: &HashMap<Generator, f64>) -> HashMap<Generator, bool> {
    Generator::ALL
        .into_iter()
        .map(|g| (g, is_satisfied(g, scores.get(&g).copied().unwrap_or(0.0))))
        .collect()
}

/// Single-generator emphasis.
///
/// A `Priority` is the lower-resolution shadow of a full ranking over
/// [`Generator`]: it captures only the **top-ranked generator**. Passed
/// through the classify API for compatibility; current `Φᵢ`
/// implementations do not change `achieved` based on priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Priority {
    Simple,
    Composable,
    #[default]
    Secure,
}

impl Priority {
    /// The generator this priority emphasizes.
    pub fn top_generator(self) -> Generator {
        match self {
            Priority::Simple => Generator::Simple,
            Priority::Composable => Generator::Composable,
            Priority::Secure => Generator::Secure,
        }
    }
}

/// Legacy per-generator metric weights for a priority/ranking.
///
/// Current `Φᵢ` implementations use fixed AND-of-raw-thresholds and do
/// not read these weights; retained for API parity with the Python
/// original.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeightProfile {
    /// Weight on cyclomatic_quality within `Φ_SIMPLE`. Entropy gets `1 - w_complexity`.
    pub w_complexity: f64,
    /// Weight on coupling_quality within `Φ_COMPOSABLE`. Instability gets `1 - w_coupling`.
    pub w_coupling: f64,
    /// Weight on taint_quality within `Φ_SECURE`. Dangerous-API reachability gets `1 - w_taint`.
    pub w_taint: f64,
}

impl WeightProfile {
    /// Look up the legacy `Priority`-keyed weight profile.
    pub fn from_priority(priority: Priority) -> WeightProfile {
        match priority {
            Priority::Simple => WeightProfile {
                w_complexity: 0.7,
                w_coupling: 0.3,
                w_taint: 0.3,
            },
            Priority::Composable => WeightProfile {
                w_complexity: 0.3,
                w_coupling: 0.7,
                w_taint: 0.3,
            },
            Priority::Secure => WeightProfile {
                w_complexity: 0.3,
                w_coupling: 0.3,
                w_taint: 0.7,
            },
        }
    }
}

/// Result of applying one policy translator `Φᵢ`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredDecision {
    /// Conservative `min(per-metric qualities)` in `[0.0, 1.0]` for
    /// display and multi-file aggregation. Does **not** gate `achieved`.
    pub score: f64,
    /// True when every supplied raw metric passes that `Φᵢ`'s policy
    /// thresholds (AND semantics). This is what
    /// `CharacteristicMorphism` feeds into `verdict_from_generators`.
    pub achieved: bool,
    /// Per-metric human-readable strings keyed by metric name (e.g.
    /// `"cfg.cyclomatic"`).
    pub interpretation: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_weight_profiles_are_in_unit_range() {
        for priority in [Priority::Simple, Priority::Composable, Priority::Secure] {
            let profile = WeightProfile::from_priority(priority);
            assert!((0.0..=1.0).contains(&profile.w_complexity));
            assert!((0.0..=1.0).contains(&profile.w_coupling));
            assert!((0.0..=1.0).contains(&profile.w_taint));
        }
    }

    #[test]
    fn meet_satisfied_uses_score_floors() {
        let scores = HashMap::from([(Generator::Simple, 0.5), (Generator::Secure, 1.0)]);
        let satisfied = meet_satisfied(&scores);
        assert!(satisfied[&Generator::Simple]); // floor is 0.40
        assert!(satisfied[&Generator::Secure]); // floor is 1.00
        assert!(!satisfied[&Generator::Composable]); // missing -> 0.0, floor is 0.80
    }
}
