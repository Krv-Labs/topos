//! `Ω` — the subobject classifier of the topos.
//!
//! This module *is* `Ω`, the subobject classifier of the topos
//! `E = Set^(C × H^op)`. Equivalently, it is the value Heyting algebra
//! `H = H(G_qual)`, the **free Heyting algebra** on the finite set of
//! quality generators
//!
//! ```text
//! G_qual = { SIMPLE, COMPOSABLE, SECURE }
//! ```
//!
//! In a topos the subobject classifier object and the internal-logic
//! Heyting algebra coincide — `Ω` carries both roles. The *characteristic
//! morphism* `χ_S : P → Ω` that maps a program into `Ω` will live in
//! `topos_core::core::characteristic_morphism` once that module
//! lands (tracked under issue #144); this file holds only the algebra
//! itself (elements, ordering, lattice operations).
//!
//! The carrier of `Ω` is the 8-element poset of all subsets of `G_qual`:
//!
//! ```text
//!                       IDEAL  (top, ⊤ = SIMPLE ∧ COMPOSABLE ∧ SECURE)
//!                      /  |  \
//!                     /   |   \
//!                    /    |    \
//!     SIMPLE_COMPOSABLE  SIMPLE_SECURE  COMPOSABLE_SECURE
//!           |  \  /             \  /  |
//!           |   \/               \/   |
//!           |   /\               /\   |
//!           |  /  \             /  \  |
//!         SIMPLE   COMPOSABLE         SECURE
//!                    \    |    /
//!                     \   |   /
//!                      \  |  /
//!                       SLOP  (bottom, ⊥)
//! ```
//!
//! The three generators are pairwise incomparable: `leq(SIMPLE, COMPOSABLE)`
//! is `false` in both directions. Meets are intersections of the satisfied
//! generator sets; `meet(SIMPLE, COMPOSABLE) == SIMPLE_COMPOSABLE` adds a
//! generator; `meet(SIMPLE_COMPOSABLE, SECURE) == IDEAL`.
//!
//! The ordering is the *partial* order of *satisfied-generator inclusion*:
//! a verdict `a` is `≤ b` iff the set of generators `a` satisfies is a
//! *superset* of the set `b` satisfies. Top (`IDEAL`) satisfies every
//! generator; bottom (`SLOP`) satisfies none. Adding a satisfied
//! constraint moves the verdict *down* toward `IDEAL`.
//!
//! The implementation uses an explicit cover relation rather than an
//! integer ordering — singletons (`SIMPLE`, `COMPOSABLE`, `SECURE`) are
//! pairwise incomparable, so the Hasse diagram is a 3-cube, not a chain.
//! [`Omega::meet`], [`Omega::join`], [`Omega::implies`], and
//! [`Omega::negation`] are computed generically from the cover, so this
//! engine works for arbitrary finite Heyting algebras — see
//! [`Omega::from_cover_relation`].
//!
//! Categorical / Rust names:
//!
//! | Math             | Rust                                          |
//! |------------------|------------------------------------------------|
//! | `Ω`              | [`Omega`]                                       |
//! | elements of `Ω`  | [`EvaluationValue`]                             |
//! | `⊤`              | [`EvaluationValue::Ideal`] / [`Omega::TOP`]     |
//! | `⊥`              | [`EvaluationValue::Slop`] / [`Omega::BOTTOM`]   |
//! | `χ_S : P → Ω`    | `CharacteristicMorphism` (pending issue #144)   |
//!
//! The top is `IDEAL` — the joint satisfaction of all generators. The
//! bottom is `SLOP`, the unconstrained universe.

use std::collections::{HashMap, HashSet};
use std::fmt;

/// The eight elements of the free Heyting algebra `H(G_qual)` on three
/// quality generators (`SIMPLE`, `COMPOSABLE`, `SECURE`).
///
/// Each value corresponds to the subset of generators a program
/// satisfies. Ordering (via [`Omega::leq`]) is by *superset of satisfied
/// generators*: `a ≤ b` iff every generator satisfied by `b` is also
/// satisfied by `a`. Thus `IDEAL = ⊤` (everything satisfied) and
/// `SLOP = ⊥` (nothing satisfied).
///
/// Encoding (discriminant = bitmask `SIMPLE|COMPOSABLE|SECURE`):
///
/// - bit 0 = `SIMPLE` satisfied
/// - bit 1 = `COMPOSABLE` satisfied
/// - bit 2 = `SECURE` satisfied
///
/// This bit ordering is just the encoding, *not* the lattice order — this
/// type intentionally does not derive `Ord`; use [`Omega::leq`] for the
/// real partial order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EvaluationValue {
    /// `⊥` — no generator satisfied. The unconstrained universe; total
    /// structural chaos.
    Slop = 0b000,
    /// Only the `SIMPLE` generator is satisfied.
    Simple = 0b001,
    /// Only the `COMPOSABLE` generator is satisfied.
    Composable = 0b010,
    /// Meet of `SIMPLE` and `COMPOSABLE`.
    SimpleComposable = 0b011,
    /// Only the `SECURE` generator is satisfied.
    Secure = 0b100,
    /// Meet of `SIMPLE` and `SECURE`.
    SimpleSecure = 0b101,
    /// Meet of `COMPOSABLE` and `SECURE`.
    ComposableSecure = 0b110,
    /// `⊤` — all three generators satisfied.
    Ideal = 0b111,
}

impl EvaluationValue {
    /// All eight elements, in ascending bitmask order.
    pub const ALL: [EvaluationValue; 8] = [
        EvaluationValue::Slop,
        EvaluationValue::Simple,
        EvaluationValue::Composable,
        EvaluationValue::SimpleComposable,
        EvaluationValue::Secure,
        EvaluationValue::SimpleSecure,
        EvaluationValue::ComposableSecure,
        EvaluationValue::Ideal,
    ];

    /// The bitmask discriminant (`SIMPLE=1, COMPOSABLE=2, SECURE=4`).
    pub fn bits(self) -> u8 {
        self as u8
    }

    /// The Python-enum-style name (`"SIMPLE_COMPOSABLE"`, etc.) — kept
    /// stable across the Rust/Python boundary for any JSON/CLI output
    /// that predates this migration.
    ///
    /// `name`/`symbol`/`description` are const lookup tables rather than
    /// three parallel 8-arm `match`es — dogfooding `topos evaluate` on
    /// this file during the migration flagged the match-statement version
    /// for cyclomatic complexity (33, exceeding the SIMPLE threshold of
    /// 15); table lookup by [`EvaluationValue::bits`] carries the same
    /// data with one branch total instead of twenty-four.
    pub fn name(self) -> &'static str {
        const NAMES: [&str; 8] = [
            "SLOP",
            "SIMPLE",
            "COMPOSABLE",
            "SIMPLE_COMPOSABLE",
            "SECURE",
            "SIMPLE_SECURE",
            "COMPOSABLE_SECURE",
            "IDEAL",
        ];
        NAMES[self.bits() as usize]
    }

    /// Unicode symbol (medal) for this verdict.
    pub fn symbol(self) -> &'static str {
        const SYMBOLS: [&str; 8] = ["❌", "🥉", "🥉", "🥈", "🥉", "🥈", "🥈", "🥇"];
        SYMBOLS[self.bits() as usize]
    }

    /// Human-readable description of this evaluation value.
    pub fn description(self) -> &'static str {
        const DESCRIPTIONS: [&str; 8] = [
            "❌ NO MEDAL - Fails every generator; unconstrained code",
            "🥉 BRONZE - Low complexity; SIMPLE generator satisfied",
            "🥉 BRONZE - Composes well; COMPOSABLE generator satisfied",
            "🥈 SILVER - SIMPLE ∧ COMPOSABLE — clean structure and clean coupling",
            "🥉 BRONZE - Safe data flow; SECURE generator satisfied",
            "🥈 SILVER - SIMPLE ∧ SECURE — clean structure and safe patterns",
            "🥈 SILVER - COMPOSABLE ∧ SECURE — well-coupled and safe patterns",
            "🥇 GOLD - Joint satisfaction of all three quality pillars",
        ];
        DESCRIPTIONS[self.bits() as usize]
    }

    /// Reconstruct a verdict from its bitmask. `None` if `bits > 0b111`.
    pub fn from_bits(bits: u8) -> Option<EvaluationValue> {
        EvaluationValue::ALL.into_iter().find(|v| v.bits() == bits)
    }
}

impl fmt::Display for EvaluationValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.symbol(), self.name())
    }
}

/// Map a satisfied-generator triple to its free-algebra verdict.
///
/// This is the concrete encoding of the truth table from `README.md`:
/// every subset of `G_qual` is a unique verdict.
pub fn verdict_from_generators(simple: bool, composable: bool, secure: bool) -> EvaluationValue {
    let bits = (simple as u8) | ((composable as u8) << 1) | ((secure as u8) << 2);
    EvaluationValue::from_bits(bits).expect("a 3-bit mask is always a valid EvaluationValue")
}

/// Direct cover relation for the default 3-cube: `value -> immediate successors`.
///
/// Each successor *adds* one satisfied generator (turns one bit on),
/// which in this order moves *down* toward `IDEAL`. `cover[a] = [b, ...]`
/// means "`b` is an immediate successor of `a`" (`a` is covered by `b`,
/// `a ≤ b`).
fn default_cover() -> HashMap<EvaluationValue, Vec<EvaluationValue>> {
    use EvaluationValue::*;
    HashMap::from([
        (Slop, vec![Simple, Composable, Secure]),
        (Simple, vec![SimpleComposable, SimpleSecure]),
        (Composable, vec![SimpleComposable, ComposableSecure]),
        (Secure, vec![SimpleSecure, ComposableSecure]),
        (SimpleComposable, vec![Ideal]),
        (SimpleSecure, vec![Ideal]),
        (ComposableSecure, vec![Ideal]),
        (Ideal, vec![]),
    ])
}

/// Raised when a lattice operation ([`Omega::meet`], [`Omega::join`], or
/// [`Omega::implies`]) has no unique answer under the supplied cover
/// relation — i.e. the cover does not actually describe a lattice.
///
/// Unreachable for [`Omega::default`]'s built-in 3-cube; only reachable
/// via a malformed [`Omega::from_cover_relation`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OmegaError {
    operation: &'static str,
    a: EvaluationValue,
    b: EvaluationValue,
}

impl fmt::Display for OmegaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot compute {} for {} and {}",
            self.operation, self.a, self.b
        )
    }
}

impl std::error::Error for OmegaError {}

/// `Ω` — the subobject classifier object of the program topos.
///
/// In the topos `E = Set^(C × H^op)` the subobject classifier coincides
/// with the value Heyting algebra `H(G_qual)`. This type carries both
/// roles: it is the truth-value object whose elements ([`EvaluationValue`])
/// are the verdicts a program can receive, *and* the Heyting algebra
/// whose operations ([`Omega::meet`], [`Omega::join`], [`Omega::implies`],
/// [`Omega::negation`]) give the internal logic of the topos.
///
/// Encodes the 3-cube Hasse diagram via an explicit cover relation. All
/// lattice operations are computed generically from the cover; no change
/// is needed if the algebra is later extended with additional generators
/// or modified by quotient relations — see [`Omega::from_cover_relation`].
pub struct Omega {
    #[allow(dead_code)]
    cover: HashMap<EvaluationValue, Vec<EvaluationValue>>,
    /// Transitive closure of `cover`: `closure[(a, b)]` iff `a ≤ b`.
    closure: HashMap<(EvaluationValue, EvaluationValue), bool>,
}

impl Omega {
    /// The least element (`⊥ = SLOP`).
    pub const BOTTOM: EvaluationValue = EvaluationValue::Slop;
    /// The greatest element (`⊤ = IDEAL`).
    pub const TOP: EvaluationValue = EvaluationValue::Ideal;

    /// Construct the lattice from direct cover relations.
    pub fn from_cover_relation(cover: HashMap<EvaluationValue, Vec<EvaluationValue>>) -> Omega {
        let mut closure = HashMap::new();
        for &value in &EvaluationValue::ALL {
            for dominated in Self::collect_dominates(&cover, value) {
                closure.insert((value, dominated), true);
            }
            closure.insert((value, value), true);
        }
        Omega { cover, closure }
    }

    fn collect_dominates(
        cover: &HashMap<EvaluationValue, Vec<EvaluationValue>>,
        value: EvaluationValue,
    ) -> HashSet<EvaluationValue> {
        let mut stack: Vec<EvaluationValue> = cover.get(&value).cloned().unwrap_or_default();
        let mut visited = HashSet::new();
        while let Some(current) = stack.pop() {
            if !visited.insert(current) {
                continue;
            }
            if let Some(next) = cover.get(&current) {
                stack.extend(next.iter().copied());
            }
        }
        visited
    }

    /// Lattice ordering: `a ≤ b`.
    pub fn leq(&self, a: EvaluationValue, b: EvaluationValue) -> bool {
        self.closure.get(&(a, b)).copied().unwrap_or(false)
    }

    /// The "and" operation (greatest lower bound).
    ///
    /// For the free Heyting algebra on quality generators, this is the
    /// intersection of satisfied-generator sets.
    pub fn meet(
        &self,
        a: EvaluationValue,
        b: EvaluationValue,
    ) -> Result<EvaluationValue, OmegaError> {
        self.resolve_bounds("meet", a, b, false)
    }

    /// The "or" operation (least upper bound).
    ///
    /// For the free Heyting algebra on quality generators, this is the
    /// union of satisfied-generator sets (i.e. the most-specific verdict
    /// that *both* `a` and `b` dominate).
    pub fn join(
        &self,
        a: EvaluationValue,
        b: EvaluationValue,
    ) -> Result<EvaluationValue, OmegaError> {
        self.resolve_bounds("join", a, b, true)
    }

    /// Intuitionistic implication (`→`).
    ///
    /// `a → b` is the largest `x` such that `a ∧ x ≤ b`.
    pub fn implies(
        &self,
        a: EvaluationValue,
        b: EvaluationValue,
    ) -> Result<EvaluationValue, OmegaError> {
        let candidates: Vec<EvaluationValue> = EvaluationValue::ALL
            .into_iter()
            .filter(|&x| self.meet(a, x).is_ok_and(|m| self.leq(m, b)))
            .collect();
        let extrema = self.select_extrema(&candidates, false);
        match extrema.as_slice() {
            [only] => Ok(*only),
            _ => Err(OmegaError {
                operation: "implies",
                a,
                b,
            }),
        }
    }

    /// Intuitionistic negation (`¬`), i.e. `a → ⊥`.
    pub fn negation(&self, a: EvaluationValue) -> Result<EvaluationValue, OmegaError> {
        self.implies(a, Self::BOTTOM)
    }

    /// Aggregate evaluation values via meet.
    ///
    /// Multi-file rollup is exactly this meet: a generator is satisfied
    /// across a codebase iff it is satisfied for every file. Returns
    /// [`Omega::TOP`] for an empty input (the empty meet is the top
    /// element, matching Heyting-algebra convention).
    ///
    /// The Python original special-cases `Mapping` inputs (aggregating a
    /// `dict`'s values); Rust's generic `IntoIterator` bound makes that
    /// unnecessary — pass `map.values().copied()` at the call site.
    pub fn aggregate<I>(&self, values: I) -> Result<EvaluationValue, OmegaError>
    where
        I: IntoIterator<Item = EvaluationValue>,
    {
        let mut iter = values.into_iter();
        let Some(mut result) = iter.next() else {
            return Ok(Self::TOP);
        };
        for value in iter {
            result = self.meet(result, value)?;
        }
        Ok(result)
    }

    /// Combine multiple evaluation values using meet (`∧`).
    pub fn combine(&self, values: &[EvaluationValue]) -> Result<EvaluationValue, OmegaError> {
        self.aggregate(values.iter().copied())
    }

    /// Check if two evaluation values are equivalent: `a ↔ b` iff
    /// `(a → b) ∧ (b → a) = ⊤`.
    ///
    /// Both `implies` calls are infallible for any well-formed lattice
    /// (there is always at least `Omega::BOTTOM` in the candidate set —
    /// see [`Omega::implies`]); the `Err` arm only guards against a
    /// malformed [`Omega::from_cover_relation`] and reports "not
    /// equivalent" rather than panicking.
    pub fn equivalent(&self, a: EvaluationValue, b: EvaluationValue) -> bool {
        match (self.implies(a, b), self.implies(b, a)) {
            (Ok(a_implies_b), Ok(b_implies_a)) => {
                self.meet(a_implies_b, b_implies_a) == Ok(Self::TOP)
            }
            _ => false,
        }
    }

    fn resolve_bounds(
        &self,
        operation: &'static str,
        a: EvaluationValue,
        b: EvaluationValue,
        maximize: bool,
    ) -> Result<EvaluationValue, OmegaError> {
        let bounds: Vec<EvaluationValue> = if maximize {
            EvaluationValue::ALL
                .into_iter()
                .filter(|&v| self.leq(a, v) && self.leq(b, v))
                .collect()
        } else {
            EvaluationValue::ALL
                .into_iter()
                .filter(|&v| self.leq(v, a) && self.leq(v, b))
                .collect()
        };
        // `join` wants the *minimal* upper bound, `meet` the *maximal*
        // lower bound — `minimal == maximize` for both cases at once.
        let candidates = self.select_extrema(&bounds, maximize);
        match candidates.as_slice() {
            [only] => Ok(*only),
            _ => Err(OmegaError { operation, a, b }),
        }
    }

    /// Select minimal or maximal elements under the partial order.
    fn select_extrema(
        &self,
        candidates: &[EvaluationValue],
        minimal: bool,
    ) -> Vec<EvaluationValue> {
        candidates
            .iter()
            .copied()
            .filter(|&c| {
                !candidates.iter().any(|&other| {
                    c != other
                        && if minimal {
                            self.leq(other, c)
                        } else {
                            self.leq(c, other)
                        }
                })
            })
            .collect()
    }
}

impl Default for Omega {
    /// The default 3-cube lattice on `{SIMPLE, COMPOSABLE, SECURE}`.
    fn default() -> Self {
        Omega::from_cover_relation(default_cover())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluation_value_order() {
        let lattice = Omega::default();
        for gen in [
            EvaluationValue::Simple,
            EvaluationValue::Composable,
            EvaluationValue::Secure,
        ] {
            assert!(lattice.leq(EvaluationValue::Slop, gen));
            assert!(lattice.leq(gen, EvaluationValue::Ideal));
        }

        let pairs = [
            (EvaluationValue::Simple, EvaluationValue::Composable),
            (EvaluationValue::Simple, EvaluationValue::Secure),
            (EvaluationValue::Composable, EvaluationValue::Secure),
        ];
        for (a, b) in pairs {
            assert!(!lattice.leq(a, b));
            assert!(!lattice.leq(b, a));
        }

        assert!(lattice.leq(EvaluationValue::Simple, EvaluationValue::SimpleComposable));
        assert!(lattice.leq(EvaluationValue::SimpleComposable, EvaluationValue::Ideal));
        assert!(!lattice.leq(EvaluationValue::SimpleComposable, EvaluationValue::Secure));
    }

    #[test]
    fn lattice_meet_join() {
        let lattice = Omega::default();
        assert_eq!(
            lattice.meet(EvaluationValue::Simple, EvaluationValue::Composable),
            Ok(EvaluationValue::Slop)
        );
        assert_eq!(
            lattice.meet(EvaluationValue::Simple, EvaluationValue::Secure),
            Ok(EvaluationValue::Slop)
        );
        assert_eq!(
            lattice.meet(EvaluationValue::Ideal, EvaluationValue::Simple),
            Ok(EvaluationValue::Simple)
        );
        assert_eq!(
            lattice.meet(EvaluationValue::Ideal, EvaluationValue::Slop),
            Ok(EvaluationValue::Slop)
        );
        assert_eq!(
            lattice.join(EvaluationValue::Simple, EvaluationValue::Composable),
            Ok(EvaluationValue::SimpleComposable)
        );
        assert_eq!(
            lattice.join(EvaluationValue::SimpleComposable, EvaluationValue::Secure),
            Ok(EvaluationValue::Ideal)
        );
        assert_eq!(
            lattice.join(EvaluationValue::Slop, EvaluationValue::Simple),
            Ok(EvaluationValue::Simple)
        );
    }

    #[test]
    fn verdict_from_generators_truth_table() {
        assert_eq!(
            verdict_from_generators(false, false, false),
            EvaluationValue::Slop
        );
        assert_eq!(
            verdict_from_generators(true, false, false),
            EvaluationValue::Simple
        );
        assert_eq!(
            verdict_from_generators(false, true, false),
            EvaluationValue::Composable
        );
        assert_eq!(
            verdict_from_generators(false, false, true),
            EvaluationValue::Secure
        );
        assert_eq!(
            verdict_from_generators(true, true, false),
            EvaluationValue::SimpleComposable
        );
        assert_eq!(
            verdict_from_generators(true, false, true),
            EvaluationValue::SimpleSecure
        );
        assert_eq!(
            verdict_from_generators(false, true, true),
            EvaluationValue::ComposableSecure
        );
        assert_eq!(
            verdict_from_generators(true, true, true),
            EvaluationValue::Ideal
        );
    }

    #[test]
    fn evaluation_value_properties() {
        assert_eq!(EvaluationValue::Ideal.symbol(), "🥇");
        assert_eq!(EvaluationValue::Slop.symbol(), "❌");
        for atom in [
            EvaluationValue::Simple,
            EvaluationValue::Composable,
            EvaluationValue::Secure,
        ] {
            assert_eq!(atom.symbol(), "🥉");
        }
        assert!(EvaluationValue::Ideal
            .description()
            .to_lowercase()
            .contains("gold"));
        assert!(EvaluationValue::Composable
            .description()
            .to_lowercase()
            .contains("composable"));
    }

    #[test]
    fn lattice_implies_and_negation() {
        let lattice = Omega::default();
        assert_eq!(
            lattice.negation(EvaluationValue::Slop),
            Ok(EvaluationValue::Ideal)
        );
        assert_eq!(
            lattice.negation(EvaluationValue::Ideal),
            Ok(EvaluationValue::Slop)
        );
        for val in EvaluationValue::ALL {
            assert!(lattice.equivalent(val, val));
        }
    }

    #[test]
    fn aggregate_empty_is_top() {
        let lattice = Omega::default();
        assert_eq!(lattice.aggregate(std::iter::empty()), Ok(Omega::TOP));
    }

    #[test]
    fn combine_matches_characteristic_morphism_examples() {
        let lattice = Omega::default();
        // meet(IDEAL, COMPOSABLE) = COMPOSABLE
        assert_eq!(
            lattice.combine(&[EvaluationValue::Ideal, EvaluationValue::Composable]),
            Ok(EvaluationValue::Composable)
        );
        // meet(SIMPLE, SECURE) = SLOP (pairwise incomparable atoms)
        assert_eq!(
            lattice.combine(&[EvaluationValue::Simple, EvaluationValue::Secure]),
            Ok(EvaluationValue::Slop)
        );
    }
}
