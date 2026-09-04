//! Shared types for the policy translators `Φᵢ : ℝ → Ω`.
//!
//! Following the math spec (§3 "Policy Translation"), each quality
//! generator `gᵢ ∈ G_qual` has an associated policy translator `Φᵢ`
//! that maps probe outputs into a [`ScoredDecision`]. The characteristic
//! morphism ([`crate::core::characteristic_morphism`]) reads each
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
//! taint flows, fan-out ≤ 10, …). `achieved` is the independent AND of
//! those checks — *not* `score ≥ score_floor(g)`. The normalized
//! `score` on [`ScoredDecision`] is `min(per-metric qualities)` for
//! reporting and multi-file meets; it does not gate `achieved`.
//!
//! [`meet_satisfied`] implements an *alternate* score-floor gate
//! (`score ≥ score_floor(g)`) for callers that already hold normalized
//! scores. The live `CharacteristicMorphism` path does **not** use it —
//! it trusts `ScoredDecision.achieved` from each `Φᵢ`.

use std::collections::{BTreeMap, HashMap};

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
    /// Default emphasis, matching the head of
    /// [`crate::evaluation::preferences::default_preferences`] and MCP's
    /// `resolve_priority(None)`. These three defaults must agree — they
    /// previously did not (engine said `Secure`, MCP said `Simple`).
    #[default]
    Simple,
    Composable,
    Secure,
    Navigable,
}

impl Priority {
    /// The generator this priority emphasizes.
    pub fn top_generator(self) -> Generator {
        match self {
            Priority::Simple => Generator::Simple,
            Priority::Composable => Generator::Composable,
            Priority::Secure => Generator::Secure,
            Priority::Navigable => Generator::Navigable,
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
    pub interpretation: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_priority_names_a_distinct_generator() {
        let generators: std::collections::HashSet<_> =
            Generator::ALL.iter().map(|g| g.as_str()).collect();
        for priority in [
            Priority::Simple,
            Priority::Composable,
            Priority::Secure,
            Priority::Navigable,
        ] {
            assert!(generators.contains(priority.top_generator().as_str()));
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
