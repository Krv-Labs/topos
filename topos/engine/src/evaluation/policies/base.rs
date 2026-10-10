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
//! - `Φ_NAVIGABLE` ↦ `policies::navigable::score_navigable`
//!
//! # Decisive semantics: AND-of-raw-metric gates
//!
//! Each `Φᵢ` owns **per-metric raw gates** (max function complexity
//! ≤ 10, zero taint flows, fan-out ≤ 10, …). `achieved` is the
//! independent AND of those checks.
//!
//! # Gate-anchored scores
//!
//! The continuous score is derived from the same gates, so it can never
//! contradict the verdict (see `docs/decisions/gate-anchored-scoring.md`):
//! each gated metric has a desirability `dᵢ` anchored so `dᵢ = TAU` at its
//! threshold, and the gate score is `G = minᵢ dᵢ`. `achieved ⇔ G ≥ TAU`.
//! Advisory metrics never enter `G`; the characteristic morphism folds them
//! in afterwards with
//! [`crate::evaluation::policies::desirability::band_score`], which moves
//! the score only within the half the verdict chose.
//!
//! [`meet_satisfied`] applies the score floor (`score ≥ TAU`) for callers
//! that already hold scores. The live `CharacteristicMorphism` path trusts
//! `ScoredDecision.achieved` from each `Φᵢ`; with gate-anchored scores the
//! two agree.

use std::collections::{BTreeMap, HashMap};

use crate::evaluation::policies::calibration::score_floor;
use crate::evaluation::policies::desirability::TAU;
use crate::evaluation::policies::gates::GateResult;
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
/// [`crate::evaluation::policies::gates`].
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
    /// Pillar score `S` in `[0.0, 1.0]`. The `Φᵢ` are advisory-agnostic,
    /// so this equals `gate_score`; the characteristic morphism applies
    /// `band_score(G, A)` once advisories are known. `S ≥ TAU` ⇔ `achieved`.
    pub score: f64,
    /// Gate score `G = minᵢ dᵢ` over the pillar's gated desirabilities.
    pub gate_score: f64,
    /// True when every supplied raw metric passes that `Φᵢ`'s policy
    /// gates (AND semantics), equivalently `gate_score ≥ TAU`. This is
    /// what `CharacteristicMorphism` feeds into `verdict_from_generators`.
    pub achieved: bool,
    /// Per-metric human-readable strings keyed by metric name (e.g.
    /// `"ast.entropy"`).
    pub interpretation: BTreeMap<String, String>,
}

impl ScoredDecision {
    /// Build a decision from a pillar's evaluated gates: `G = minᵢ dᵢ`,
    /// `achieved` = every gate passed, `score = G`. No gates (nothing
    /// measured) is a vacuous pass with score `1.0`.
    pub fn from_gates(results: &[GateResult]) -> Self {
        Self::from_gates_with(results, GateResult::desirability)
    }

    /// As [`Self::from_gates`], with a per-gate desirability override (the
    /// SIMPLE tiny-file entropy floor). The override must keep
    /// `d ≥ TAU` ⇔ `passed()`.
    pub fn from_gates_with(results: &[GateResult], d: impl Fn(&GateResult) -> f64) -> Self {
        let gate_score = results.iter().map(d).fold(1.0, f64::min);
        let achieved = results.iter().all(GateResult::passed);
        debug_assert_eq!(achieved, gate_score >= TAU, "score contradicts verdict");
        ScoredDecision {
            score: gate_score,
            gate_score,
            achieved,
            interpretation: results
                .iter()
                .map(|r| (r.spec.metric.to_string(), r.interpretation()))
                .collect(),
        }
    }
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
        let scores = HashMap::from([(Generator::Simple, 0.5), (Generator::Secure, 0.49)]);
        let satisfied = meet_satisfied(&scores);
        assert!(satisfied[&Generator::Simple]); // floor is TAU = 0.5
        assert!(!satisfied[&Generator::Secure]);
        assert!(!satisfied[&Generator::Composable]); // missing -> 0.0
    }
}
