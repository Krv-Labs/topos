//! User preferences over the quality generators — an induced strict total
//! order on `Ω`.
//!
//! [`crate::evaluation::policies::base::Priority`] is a *single* upweighted
//! generator: a knob on the policy translators `Φᵢ`. This module is a
//! strictly stronger statement of the operator's intent — a **strict total
//! order on the four generators**:
//!
//! ```text
//! g₁ ≻ g₂ ≻ g₃ ≻ g₄   with   {g₁, g₂, g₃, g₄} = G_qual
//! ```
//!
//! `Ω = H(G_qual)` (see [`crate::core::omega`]) is only *partially* ordered
//! — the generator atoms `SIMPLE`, `COMPOSABLE`, `SECURE`, `NAVIGABLE` are
//! pairwise incomparable under the Heyting order `≤_H`. A
//! [`UserPreferences`] ranking *linearizes* that partial order into a
//! strict total order `⪯_r` by scoring each verdict `v ∈ Ω` on its
//! satisfied-generator bitmask, weighted by preference rank:
//!
//! ```text
//! score(v) = Σᵢ 2^(n − i) · ⟦gᵢ satisfied by v⟧
//! ```
//!
//! For the default ranking `(SIMPLE, COMPOSABLE, SECURE, NAVIGABLE)`
//! (most → least preferred), that's weights `8 / 4 / 2 / 1`:
//!
//! ```text
//! IDEAL                       = 8 + 4 + 2 + 1 = 15
//! SIMPLE_COMPOSABLE_SECURE    = 8 + 4 + 2     = 14
//! SIMPLE_COMPOSABLE_NAVIGABLE = 8 + 4     + 1 = 13
//! SIMPLE_COMPOSABLE           = 8 + 4         = 12  <- fallback ("ideal ∩")
//! SIMPLE_SECURE_NAVIGABLE     = 8     + 2 + 1 = 11
//! ...
//! NAVIGABLE                   =             1 =  1
//! SLOP                                        =  0
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
//!    intersection"): guarantee the two pillars the operator cares most
//!    about, concede the rest. When `IDEAL` plateaus after a few refactor
//!    iterations, the agent diverts here. For ranking `(SIMPLE, COMPOSABLE,
//!    SECURE, NAVIGABLE)` the fallback is `SIMPLE_COMPOSABLE`; for
//!    `(COMPOSABLE, SECURE, SIMPLE, NAVIGABLE)` it is `COMPOSABLE_SECURE`.
//!
//!    Note this is *not* the element directly below `IDEAL` in the walk —
//!    that one concedes only the single least-preferred generator. The two
//!    coincided while `G_qual` had three generators and diverged when
//!    `NAVIGABLE` made it four.
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

use crate::core::omega::{verdict_from_generators, EvaluationValue, GENERATOR_COUNT};

pub use crate::core::omega::Generator;

/// How many generators a ranking must list — every one of them.
pub const RANKING_LEN: usize = GENERATOR_COUNT as usize;

/// Whether `value` (an element of `Ω`) satisfies generator `g`.
fn generator_satisfied(value: EvaluationValue, g: Generator) -> bool {
    value.bits() & g.value().bits() != 0
}

/// A ranking supplied to [`UserPreferences::new`]/[`UserPreferences::with_target`]
/// was not a permutation of the four [`Generator`]s (e.g. a repeat).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidRanking {
    ranking: [Generator; RANKING_LEN],
}

impl fmt::Display for InvalidRanking {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ranking must be a permutation of {{simple, composable, secure, navigable}}, got {:?}",
            self.ranking
        )
    }
}

impl std::error::Error for InvalidRanking {}

/// A strict total order on `G_qual` — one operator's/agent's preference
/// ranking over the four quality generators, and the `Ω`-targeting policy
/// it induces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserPreferences {
    /// Four distinct generators, most-preferred first.
    ranking: [Generator; RANKING_LEN],
    /// Explicit aspirational-target override. `None` means "default to
    /// `IDEAL`" — see [`UserPreferences::aspirational_target`].
    target: Option<EvaluationValue>,
}

impl UserPreferences {
    /// Construct from a full ranking; the aspirational target defaults to
    /// `IDEAL`.
    ///
    /// Errors with [`InvalidRanking`] unless `ranking` is a permutation of
    /// the four generators.
    pub fn new(ranking: [Generator; RANKING_LEN]) -> Result<UserPreferences, InvalidRanking> {
        Self::with_target(ranking, None)
    }

    /// Construct with an explicit aspirational-target override — for a
    /// caller who knows a priori that `IDEAL` is unreachable for this
    /// codebase.
    pub fn with_target(
        ranking: [Generator; RANKING_LEN],
        target: Option<EvaluationValue>,
    ) -> Result<UserPreferences, InvalidRanking> {
        // A permutation iff no generator repeats, since the array length
        // already equals |G_qual|.
        let distinct = ranking.iter().collect::<std::collections::HashSet<_>>();
        if distinct.len() != RANKING_LEN {
            return Err(InvalidRanking { ranking });
        }
        Ok(UserPreferences { ranking, target })
    }

    /// The ranking, most-preferred first.
    pub fn ranking(&self) -> [Generator; RANKING_LEN] {
        self.ranking
    }

    // Induced ordering ----------------------------------------------------

    /// Lex-weighted preference score for a verdict. Higher is more
    /// preferred.
    ///
    /// Weights halve down the ranking (`8 / 4 / 2 / 1`) so each generator
    /// dominates every lower-ranked one combined — strictly lexicographic
    /// on the satisfied-generator bits in preference order.
    pub fn score(&self, value: EvaluationValue) -> u32 {
        self.ranking
            .iter()
            .enumerate()
            .filter(|(_, &g)| generator_satisfied(value, g))
            .map(|(rank, _)| 1u32 << (RANKING_LEN - 1 - rank))
            .sum()
    }

    /// All sixteen verdicts sorted by descending preference score.
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
        verdict_from_generators(&self.ranking[..2])
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

/// Conservative default: `SIMPLE ≻ COMPOSABLE ≻ SECURE ≻ NAVIGABLE`.
///
/// Simplicity comes first (the cheapest property to verify and currently
/// our strongest measure), then composability (the most cross-cutting, and
/// the only one requiring an external dependency graph), then security,
/// then navigability — ranked last for now because its threshold is still
/// provisional, so it should be the first thing a relaxation walk gives
/// up on.
pub fn default_preferences() -> UserPreferences {
    UserPreferences::new([
        Generator::Simple,
        Generator::Composable,
        Generator::Secure,
        Generator::Navigable,
    ])
    .expect("(SIMPLE, COMPOSABLE, SECURE, NAVIGABLE) is trivially a permutation")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::omega::Omega;
    use std::collections::HashSet;

    const DEFAULT_RANKING: [Generator; RANKING_LEN] = [
        Generator::Simple,
        Generator::Composable,
        Generator::Secure,
        Generator::Navigable,
    ];

    fn prefs(ranking: [Generator; RANKING_LEN]) -> UserPreferences {
        UserPreferences::new(ranking).unwrap()
    }

    /// All 24 permutations of `G_qual` — small enough to assert over
    /// exhaustively, which beats spot-checking three of them.
    fn all_rankings() -> Vec<[Generator; RANKING_LEN]> {
        let mut out = Vec::new();
        for a in Generator::ALL {
            for b in Generator::ALL {
                for c in Generator::ALL {
                    for d in Generator::ALL {
                        let ranking = [a, b, c, d];
                        if UserPreferences::new(ranking).is_ok() {
                            out.push(ranking);
                        }
                    }
                }
            }
        }
        assert_eq!(out.len(), 24);
        out
    }

    #[test]
    fn ranking_must_be_permutation() {
        let result = UserPreferences::new([
            Generator::Simple,
            Generator::Simple,
            Generator::Secure,
            Generator::Navigable,
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn aspirational_target_is_ideal_by_default() {
        let p = prefs(DEFAULT_RANKING);
        assert_eq!(p.aspirational_target(), EvaluationValue::Ideal);
    }

    #[test]
    fn fallback_target_is_top_two_meet() {
        let p = prefs(DEFAULT_RANKING);
        assert_eq!(p.fallback_target(), EvaluationValue::SimpleComposable);

        let p = prefs([
            Generator::Secure,
            Generator::Simple,
            Generator::Composable,
            Generator::Navigable,
        ]);
        assert_eq!(p.fallback_target(), EvaluationValue::SimpleSecure);

        let p = prefs([
            Generator::Navigable,
            Generator::Composable,
            Generator::Secure,
            Generator::Simple,
        ]);
        assert_eq!(p.fallback_target(), EvaluationValue::ComposableNavigable);
    }

    /// Pins the exact table published in `topos://docs/preferences`. If
    /// this fails, that doc is now wrong and must be updated with it.
    #[test]
    fn default_ranking_induced_order_matches_the_published_table() {
        use EvaluationValue::*;
        let p = prefs(DEFAULT_RANKING);
        let expected = [
            (Ideal, 15),
            (SimpleComposableSecure, 14),
            (SimpleComposableNavigable, 13),
            (SimpleComposable, 12),
            (SimpleSecureNavigable, 11),
            (SimpleSecure, 10),
            (SimpleNavigable, 9),
            (Simple, 8),
            (ComposableSecureNavigable, 7),
            (ComposableSecure, 6),
            (ComposableNavigable, 5),
            (Composable, 4),
            (SecureNavigable, 3),
            (Secure, 2),
            (Navigable, 1),
            (Slop, 0),
        ];
        let order = p.induced_total_order();
        for (index, (value, score)) in expected.into_iter().enumerate() {
            assert_eq!(order[index], value, "position {index}");
            assert_eq!(p.score(value), score, "score of {value}");
        }
        assert_eq!(p.fallback_target(), SimpleComposable);
        assert_eq!(p.next_step(Secure), Some(SecureNavigable));
    }

    /// The fallback is the meet of the top two ranked generators for
    /// *every* ranking, not just the three spot-checked above.
    #[test]
    fn fallback_target_is_top_two_meet_for_all_rankings() {
        for ranking in all_rankings() {
            let p = prefs(ranking);
            assert_eq!(
                p.fallback_target(),
                verdict_from_generators(&ranking[..2]),
                "{ranking:?}"
            );
        }
    }

    #[test]
    fn explicit_target_override() {
        let p =
            UserPreferences::with_target(DEFAULT_RANKING, Some(EvaluationValue::SimpleComposable))
                .unwrap();
        assert_eq!(p.aspirational_target(), EvaluationValue::SimpleComposable);
    }

    #[test]
    fn induced_order_is_lex_on_weights() {
        let p = prefs(DEFAULT_RANKING);
        let order = p.induced_total_order();
        // Weights 8/4/2/1: the order is the descending binary count on the
        // satisfied-generator bits, read in preference order.
        assert_eq!(order[0], EvaluationValue::Ideal);
        assert_eq!(order[1], EvaluationValue::SimpleComposableSecure);
        assert_eq!(order[2], EvaluationValue::SimpleComposableNavigable);
        assert_eq!(order[3], EvaluationValue::SimpleComposable);
        assert_eq!(order[4], EvaluationValue::SimpleSecureNavigable);
        assert_eq!(*order.last().unwrap(), EvaluationValue::Slop);
    }

    /// The induced order must be a strict total order — no ties — for
    /// every ranking, or the relaxation walk has no unique next step.
    #[test]
    fn induced_order_is_total_for_every_ranking() {
        for ranking in all_rankings() {
            let p = prefs(ranking);
            let scores: HashSet<u32> = EvaluationValue::ALL.iter().map(|&v| p.score(v)).collect();
            assert_eq!(scores.len(), EvaluationValue::ALL.len(), "{ranking:?}");
        }
    }

    #[test]
    fn induced_order_refines_heyting() {
        // a ≤_H b ⟹ a ⪯_r b for any ranking.
        let omega = Omega::default();
        for ranking in all_rankings() {
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
        let p = prefs(DEFAULT_RANKING);
        let walk = p.relaxation_walk(Some(EvaluationValue::Slop));
        // IDEAL is the aspirational target — first in the walk.
        assert_eq!(walk[0], EvaluationValue::Ideal);
        // One step below IDEAL is "drop only the least-preferred pillar".
        assert_eq!(walk[1], EvaluationValue::SimpleComposableSecure);
        // The fallback target — guarantee the top two, concede the rest —
        // is further down the walk, not adjacent to IDEAL. At three
        // generators those two elements coincided; at four they do not.
        assert!(walk.contains(&p.fallback_target()));
        assert!(p.score(p.fallback_target()) < p.score(walk[1]));
    }

    #[test]
    fn relaxation_walk_stops_above_current() {
        let p = prefs(DEFAULT_RANKING);
        let walk = p.relaxation_walk(Some(EvaluationValue::Simple));
        // All walk entries must outrank the current verdict.
        for &v in &walk {
            assert!(p.score(v) > p.score(EvaluationValue::Simple));
        }
        // IDEAL and the fallback both included.
        assert!(walk.contains(&EvaluationValue::Ideal));
        assert!(walk.contains(&EvaluationValue::SimpleComposable));
        // Anything without SIMPLE ranks below SIMPLE under weights
        // 8/4/2/1, so none of it appears in the walk.
        assert!(!walk.contains(&EvaluationValue::Secure));
        assert!(!walk.contains(&EvaluationValue::ComposableSecureNavigable));
        assert!(!walk.contains(&EvaluationValue::Slop));
    }

    #[test]
    fn relaxation_walk_empty_at_ideal() {
        let p = prefs(DEFAULT_RANKING);
        assert!(p.relaxation_walk(Some(EvaluationValue::Ideal)).is_empty());
        assert_eq!(p.next_step(EvaluationValue::Ideal), None);
    }

    #[test]
    fn next_step_is_smallest_improvement() {
        let p = prefs(DEFAULT_RANKING);
        // From SLOP the smallest improvement is the lowest-ranked verdict
        // strictly above SLOP — which is NAVIGABLE (weight 1, ranked last).
        assert_eq!(
            p.next_step(EvaluationValue::Slop),
            Some(EvaluationValue::Navigable)
        );
        // From NAVIGABLE -> SECURE (weight 2).
        assert_eq!(
            p.next_step(EvaluationValue::Navigable),
            Some(EvaluationValue::Secure)
        );
    }

    #[test]
    fn progress_reaches_one_at_ideal() {
        let p = prefs(DEFAULT_RANKING);
        assert_eq!(p.progress(EvaluationValue::Slop), 0.0);
        assert_eq!(p.progress(EvaluationValue::Ideal), 1.0);
        // Fallback target is partial progress (12/15 with weights 8+4+2+1).
        let mid = p.progress(EvaluationValue::SimpleComposable);
        assert!(mid > 0.75 && mid < 1.0);
    }

    #[test]
    fn default_preferences_ranking_and_targets() {
        let p = default_preferences();
        assert_eq!(p.ranking()[0], Generator::Simple);
        assert_eq!(p.aspirational_target(), EvaluationValue::Ideal);
        assert_eq!(p.fallback_target(), EvaluationValue::SimpleComposable);
    }
}
