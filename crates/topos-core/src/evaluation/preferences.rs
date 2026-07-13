//! User preferences over the quality generators — an induced strict total
//! order on `Ω`.
//!
//! [`crate::evaluation::policies::base::Priority`] is a *single* upweighted
//! generator: a knob on the policy translators `Φᵢ`. This module is a
//! strictly stronger statement of the operator's intent — a **strict total
//! order on the three generators**:
//!
//! ```text
//! g₁ ≻ g₂ ≻ g₃   with   {g₁, g₂, g₃} = G_qual
//! ```
//!
//! `Ω = H(G_qual)` (see [`crate::core::omega`]) is only *partially* ordered
//! — the three generator atoms `SIMPLE`, `COMPOSABLE`, `SECURE` are pairwise
//! incomparable under the Heyting order `≤_H`. A [`UserPreferences`] ranking
//! *linearizes* that partial order into a strict total order `⪯_r` by
//! scoring each verdict `v ∈ Ω` on its satisfied-generator bitmask, weighted
//! by preference rank:
//!
//! ```text
//! score(v) = Σᵢ 2^(n − i) · ⟦gᵢ satisfied by v⟧
//! ```
//!
//! For the default ranking `(SIMPLE, COMPOSABLE, SECURE)` (most → least
//! preferred), that's weights `4 / 2 / 1`:
//!
//! ```text
//! IDEAL              = 4 + 2 + 1 = 7
//! SIMPLE_COMPOSABLE  = 4 + 2     = 6   <- fallback target ("ideal ∩")
//! SIMPLE_SECURE      = 4 + 1     = 5
//! SIMPLE             = 4
//! COMPOSABLE_SECURE  =     2 + 1 = 3
//! COMPOSABLE         =     2
//! SECURE             =         1
//! SLOP               = 0
//! ```
//!
//! This *refines* the Heyting order — `a ≤_H b ⟹ a ⪯_r b` (see the
//! `induced_order_refines_heyting` test below) — and, crucially,
//! disambiguates the three places where `≤_H` leaves atoms incomparable.
//!
//! # Two-stage targeting: aspirational target, then pragmatic fallback
//!
//! An agent driving the relaxation walk targets `Ω` in two stages:
//!
//! 1. **Aspirational target** ([`UserPreferences::aspirational_target`]) —
//!    `IDEAL` by default. Topos does not assume `IDEAL` is unreachable a
//!    priori; some files genuinely satisfy every generator.
//! 2. **Pragmatic fallback** ([`UserPreferences::fallback_target`]) — the
//!    Heyting *meet* of the top-two ranked generators (the "ideal
//!    intersection"). When `IDEAL` plateaus after a few refactor
//!    iterations, the agent diverts here. For ranking `(SIMPLE, COMPOSABLE,
//!    SECURE)` the fallback is `SIMPLE_COMPOSABLE`; for `(COMPOSABLE,
//!    SECURE, SIMPLE)` it is `COMPOSABLE_SECURE`.
//!
//! # Relaxation walk
//!
//! [`UserPreferences::relaxation_walk`] returns the descending sequence of
//! verdicts from the aspirational target down to (but not including) the
//! current verdict — the **targeted relaxation walk**. An agent uses it to
//! pick the next achievable goal one step at a time; the fallback target
//! sits exactly one step below `IDEAL` in this walk, which is what makes it
//! the natural divert point when `IDEAL` plateaus.
//! [`UserPreferences::next_step`] takes the bottom of the walk (the
//! smallest achievable improvement); [`UserPreferences::progress`] reports
//! fractional progress toward the aspirational target.
//!
//! # Deviation from the Python original
//!
//! Python's `UserPreferences.__post_init__` raises `ValueError` for a
//! malformed ranking — i.e. it validates *after* field assignment, since a
//! dataclass has no other hook. A plain Rust struct literal has no
//! equivalent post-construction hook, so validation happens *before*
//! construction here: [`UserPreferences::new`] and
//! [`UserPreferences::with_target`] return `Result<_, InvalidRanking>`, and
//! there is no public way to name an invalid [`UserPreferences`] value.

use std::fmt;

use crate::core::omega::{verdict_from_generators, EvaluationValue};

/// The three quality generators of `G_qual`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Generator {
    Simple,
    Composable,
    Secure,
}

impl Generator {
    pub const ALL: [Generator; 3] = [Generator::Simple, Generator::Composable, Generator::Secure];

    pub fn as_str(self) -> &'static str {
        match self {
            Generator::Simple => "simple",
            Generator::Composable => "composable",
            Generator::Secure => "secure",
        }
    }

    /// This generator's bit in the [`EvaluationValue`] encoding
    /// (`SIMPLE=0b001, COMPOSABLE=0b010, SECURE=0b100` — see
    /// [`crate::core::omega::EvaluationValue`]'s doc comment for the
    /// canonical statement of this encoding).
    fn bit(self) -> u8 {
        match self {
            Generator::Simple => 0b001,
            Generator::Composable => 0b010,
            Generator::Secure => 0b100,
        }
    }
}

/// Whether `value` (an element of `Ω`) satisfies generator `g`.
fn generator_satisfied(value: EvaluationValue, g: Generator) -> bool {
    value.bits() & g.bit() != 0
}

/// A ranking supplied to [`UserPreferences::new`]/[`UserPreferences::with_target`]
/// was not a permutation of the three [`Generator`]s (e.g. a repeat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidRanking {
    ranking: [Generator; 3],
}

impl fmt::Display for InvalidRanking {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ranking must be a permutation of {{simple, composable, secure}}, got {:?}",
            self.ranking
        )
    }
}

impl std::error::Error for InvalidRanking {}

/// A strict total order on `G_qual` — one operator's/agent's preference
/// ranking over the three quality generators, and the `Ω`-targeting policy
/// it induces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserPreferences {
    /// Three distinct generators, most-preferred first.
    ranking: [Generator; 3],
    /// Explicit aspirational-target override. `None` means "default to
    /// `IDEAL`" — see [`UserPreferences::aspirational_target`].
    target: Option<EvaluationValue>,
}

impl UserPreferences {
    /// Construct from a full ranking; the aspirational target defaults to
    /// `IDEAL`.
    ///
    /// Errors with [`InvalidRanking`] unless `ranking` is a permutation of
    /// the three generators.
    pub fn new(ranking: [Generator; 3]) -> Result<UserPreferences, InvalidRanking> {
        Self::with_target(ranking, None)
    }

    /// Construct with an explicit aspirational-target override — for a
    /// caller who knows a priori that `IDEAL` is unreachable for this
    /// codebase.
    pub fn with_target(
        ranking: [Generator; 3],
        target: Option<EvaluationValue>,
    ) -> Result<UserPreferences, InvalidRanking> {
        let [g1, g2, g3] = ranking;
        let is_permutation = g1 != g2 && g1 != g3 && g2 != g3;
        if !is_permutation {
            return Err(InvalidRanking { ranking });
        }
        Ok(UserPreferences { ranking, target })
    }

    /// The ranking, most-preferred first.
    pub fn ranking(&self) -> [Generator; 3] {
        self.ranking
    }

    // Induced ordering ----------------------------------------------------

    /// Lex-weighted preference score for a verdict. Higher is more
    /// preferred.
    ///
    /// Weights are `4 / 2 / 1` across the ranking so the top-ranked
    /// generator dominates the next two combined — strictly lexicographic
    /// on the satisfied-generator bits in preference order.
    pub fn score(&self, value: EvaluationValue) -> u32 {
        const WEIGHTS: [u32; 3] = [4, 2, 1];
        self.ranking
            .iter()
            .zip(WEIGHTS)
            .map(|(&g, w)| if generator_satisfied(value, g) { w } else { 0 })
            .sum()
    }

    /// All eight verdicts sorted by descending preference score.
    ///
    /// Uses a stable sort, so tied verdicts keep [`EvaluationValue::ALL`]'s
    /// ascending-bitmask order — matching Python's `sorted(..., reverse=True)`,
    /// which is also stable (equal keys keep their original relative order;
    /// `reverse` flips the comparison, not the tie groups).
    pub fn induced_total_order(&self) -> Vec<EvaluationValue> {
        let mut order = EvaluationValue::ALL.to_vec();
        order.sort_by_key(|&v| std::cmp::Reverse(self.score(v)));
        order
    }

    // Target + relaxation walk --------------------------------------------

    /// The first target the agent should attempt.
    ///
    /// Defaults to `IDEAL` (beat the policy thresholds for all three
    /// generators). Overridden via [`UserPreferences::with_target`] if the
    /// caller knows a priori that `IDEAL` is unreachable for this codebase.
    pub fn aspirational_target(&self) -> EvaluationValue {
        self.target.unwrap_or(EvaluationValue::Ideal)
    }

    /// The pragmatic divert-point if `IDEAL` plateaus.
    ///
    /// This is the meet of the top-two ranked generators — the "ideal
    /// intersection". For ranking `(COMPOSABLE, SECURE, SIMPLE)` this is
    /// `COMPOSABLE_SECURE`; for `(SIMPLE, COMPOSABLE, SECURE)` it is
    /// `SIMPLE_COMPOSABLE`.
    pub fn fallback_target(&self) -> EvaluationValue {
        let (g1, g2) = (self.ranking[0], self.ranking[1]);
        verdict_from_generators(
            g1 == Generator::Simple || g2 == Generator::Simple,
            g1 == Generator::Composable || g2 == Generator::Composable,
            g1 == Generator::Secure || g2 == Generator::Secure,
        )
    }

    /// Alias for [`UserPreferences::aspirational_target`] — the "resolved"
    /// target is what the agent aims at on iteration 1. Always `IDEAL`
    /// unless overridden.
    pub fn resolved_target(&self) -> EvaluationValue {
        self.aspirational_target()
    }

    /// Descending walk from the aspirational target toward `current`.
    ///
    /// Returned in descending preference order, starting at the
    /// aspirational target (default `IDEAL`) and ending one step above
    /// `current`. The **second** element of the walk (when `current` is
    /// `SLOP`, or when `current` is `None`) is the
    /// [`UserPreferences::fallback_target`] — the natural divert point when
    /// `IDEAL` proves unreachable.
    ///
    /// `current: None` returns the full descending walk down to (and
    /// including) `SLOP`. Empty when `current` already meets or exceeds the
    /// target.
    pub fn relaxation_walk(&self, current: Option<EvaluationValue>) -> Vec<EvaluationValue> {
        let target = self.aspirational_target();
        let target_score = self.score(target);
        let descending: Vec<EvaluationValue> = self
            .induced_total_order()
            .into_iter()
            .filter(|&v| self.score(v) <= target_score)
            .collect();

        let Some(current) = current else {
            return descending;
        };
        let current_score = self.score(current);
        if current_score >= target_score {
            return Vec::new();
        }
        descending
            .into_iter()
            .filter(|&v| self.score(v) > current_score)
            .collect()
    }

    /// The immediate next achievable verdict above `current`.
    ///
    /// The bottom of the relaxation walk — the smallest improvement that
    /// still respects the preference order. `None` when at or beyond the
    /// aspirational target.
    pub fn next_step(&self, current: EvaluationValue) -> Option<EvaluationValue> {
        self.relaxation_walk(Some(current)).into_iter().last()
    }

    /// Fractional progress from `SLOP` to the aspirational target, in
    /// `[0.0, 1.0]`. Reaches `1.0` exactly at the target verdict.
    pub fn progress(&self, current: EvaluationValue) -> f64 {
        let target_score = self.score(self.aspirational_target());
        if target_score == 0 {
            return 1.0;
        }
        (self.score(current) as f64 / target_score as f64).min(1.0)
    }
}

/// Conservative default: `SIMPLE ≻ COMPOSABLE ≻ SECURE`.
///
/// Simplicity comes first (the cheapest property to verify and currently
/// our strongest measure), then composability (the most cross-cutting, and
/// the only one requiring an external dependency graph), then security.
pub fn default_preferences() -> UserPreferences {
    UserPreferences::new([Generator::Simple, Generator::Composable, Generator::Secure])
        .expect("(SIMPLE, COMPOSABLE, SECURE) is trivially a permutation")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::omega::Omega;

    fn prefs(ranking: [Generator; 3]) -> UserPreferences {
        UserPreferences::new(ranking).unwrap()
    }

    #[test]
    fn ranking_must_be_permutation() {
        let result =
            UserPreferences::new([Generator::Simple, Generator::Simple, Generator::Secure]);
        assert!(result.is_err());
    }

    #[test]
    fn aspirational_target_is_ideal_by_default() {
        let p = prefs([Generator::Simple, Generator::Composable, Generator::Secure]);
        assert_eq!(p.aspirational_target(), EvaluationValue::Ideal);
    }

    #[test]
    fn fallback_target_is_top_two_meet() {
        let p = prefs([Generator::Simple, Generator::Composable, Generator::Secure]);
        assert_eq!(p.fallback_target(), EvaluationValue::SimpleComposable);

        let p = prefs([Generator::Secure, Generator::Simple, Generator::Composable]);
        assert_eq!(p.fallback_target(), EvaluationValue::SimpleSecure);

        let p = prefs([Generator::Composable, Generator::Secure, Generator::Simple]);
        assert_eq!(p.fallback_target(), EvaluationValue::ComposableSecure);
    }

    #[test]
    fn explicit_target_override() {
        let p = UserPreferences::with_target(
            [Generator::Simple, Generator::Composable, Generator::Secure],
            Some(EvaluationValue::SimpleComposable),
        )
        .unwrap();
        assert_eq!(p.aspirational_target(), EvaluationValue::SimpleComposable);
    }

    #[test]
    fn induced_order_is_lex_on_weights() {
        let p = prefs([Generator::Simple, Generator::Composable, Generator::Secure]);
        let order = p.induced_total_order();
        // Highest = IDEAL, then SIMPLE_COMPOSABLE, then SIMPLE_SECURE, …
        assert_eq!(order[0], EvaluationValue::Ideal);
        assert_eq!(order[1], EvaluationValue::SimpleComposable);
        assert_eq!(order[2], EvaluationValue::SimpleSecure);
        assert_eq!(order[3], EvaluationValue::Simple);
        assert_eq!(*order.last().unwrap(), EvaluationValue::Slop);
    }

    #[test]
    fn induced_order_refines_heyting() {
        // a ≤_H b ⟹ a ⪯_r b for any ranking.
        let omega = Omega::default();
        for ranking in [
            [Generator::Simple, Generator::Composable, Generator::Secure],
            [Generator::Secure, Generator::Composable, Generator::Simple],
            [Generator::Composable, Generator::Simple, Generator::Secure],
        ] {
            let p = prefs(ranking);
            for &a in &EvaluationValue::ALL {
                for &b in &EvaluationValue::ALL {
                    if omega.leq(a, b) {
                        assert!(p.score(a) <= p.score(b));
                    }
                }
            }
        }
    }

    #[test]
    fn relaxation_walk_starts_at_ideal_then_fallback() {
        let p = prefs([Generator::Simple, Generator::Composable, Generator::Secure]);
        let walk = p.relaxation_walk(Some(EvaluationValue::Slop));
        // IDEAL is the aspirational target — first in the walk.
        assert_eq!(walk[0], EvaluationValue::Ideal);
        // The "divert" element directly below IDEAL is the fallback target.
        assert_eq!(walk[1], EvaluationValue::SimpleComposable);
        assert_eq!(walk[1], p.fallback_target());
    }

    #[test]
    fn relaxation_walk_stops_above_current() {
        let p = prefs([Generator::Simple, Generator::Composable, Generator::Secure]);
        let walk = p.relaxation_walk(Some(EvaluationValue::Simple));
        // All walk entries must outrank the current verdict.
        for &v in &walk {
            assert!(p.score(v) > p.score(EvaluationValue::Simple));
        }
        // IDEAL and the fallback both included.
        assert!(walk.contains(&EvaluationValue::Ideal));
        assert!(walk.contains(&EvaluationValue::SimpleComposable));
        // SECURE / COMPOSABLE_SECURE / COMPOSABLE / SLOP rank below SIMPLE.
        assert!(!walk.contains(&EvaluationValue::Secure));
        assert!(!walk.contains(&EvaluationValue::Slop));
    }

    #[test]
    fn relaxation_walk_empty_at_ideal() {
        let p = prefs([Generator::Simple, Generator::Composable, Generator::Secure]);
        assert!(p.relaxation_walk(Some(EvaluationValue::Ideal)).is_empty());
        assert_eq!(p.next_step(EvaluationValue::Ideal), None);
    }

    #[test]
    fn next_step_is_smallest_improvement() {
        let p = prefs([Generator::Simple, Generator::Composable, Generator::Secure]);
        // From SLOP the smallest improvement is the lowest-ranked verdict
        // strictly above SLOP — which is SECURE (weight 1).
        assert_eq!(
            p.next_step(EvaluationValue::Slop),
            Some(EvaluationValue::Secure)
        );
        // From SECURE -> COMPOSABLE (weight 2).
        assert_eq!(
            p.next_step(EvaluationValue::Secure),
            Some(EvaluationValue::Composable)
        );
    }

    #[test]
    fn progress_reaches_one_at_ideal() {
        let p = prefs([Generator::Simple, Generator::Composable, Generator::Secure]);
        assert_eq!(p.progress(EvaluationValue::Slop), 0.0);
        assert_eq!(p.progress(EvaluationValue::Ideal), 1.0);
        // Fallback target is partial progress (6/7 with weights 4+2+1).
        let mid = p.progress(EvaluationValue::SimpleComposable);
        assert!(mid > 0.8 && mid < 1.0);
    }

    #[test]
    fn default_preferences_ranking_and_targets() {
        let p = default_preferences();
        assert_eq!(p.ranking()[0], Generator::Simple);
        assert_eq!(p.aspirational_target(), EvaluationValue::Ideal);
        assert_eq!(p.fallback_target(), EvaluationValue::SimpleComposable);
    }
}
