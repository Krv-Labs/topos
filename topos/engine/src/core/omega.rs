//! `Ω` — the subobject classifier of the topos.
//!
//! This module *is* `Ω`, the subobject classifier of the topos
//! `E = Set^(C × H^op)`. Equivalently, it is the value Heyting algebra
//! `H = H(G_qual)`, the **free Heyting algebra** on the finite set of
//! quality generators
//!
//! ```text
//! G_qual = { SIMPLE, COMPOSABLE, SECURE, NAVIGABLE }
//! ```
//!
//! In a topos the subobject classifier object and the internal-logic
//! Heyting algebra coincide — `Ω` carries both roles. The *characteristic
//! morphism* `χ_S : P → Ω` that maps a program into `Ω` lives in
//! [`crate::core::characteristic_morphism`]; this file holds only the
//! algebra itself (elements, ordering, lattice operations).
//!
//! The carrier of `Ω` is the 16-element poset of all subsets of `G_qual`
//! — a 4-cube. The 3-generator sub-cube (everything with `NAVIGABLE`
//! unsatisfied) keeps exactly the shape it had before the fourth
//! generator landed:
//!
//! ```text
//!         SIMPLE_COMPOSABLE_SECURE  (⊤ of the NAVIGABLE-free sub-cube)
//!                      /  |  \
//!                     /   |   \
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
//! and `IDEAL = ⊤` sits one level above `SIMPLE_COMPOSABLE_SECURE`, with
//! a parallel copy of the whole diagram hanging off `NAVIGABLE`.
//!
//! The four generators are pairwise incomparable: `leq(SIMPLE, COMPOSABLE)`
//! is `false` in both directions. Meets are intersections of the satisfied
//! generator sets; `meet(SIMPLE, COMPOSABLE) == SIMPLE_COMPOSABLE` adds a
//! generator; `meet(SIMPLE_COMPOSABLE_SECURE, NAVIGABLE) == IDEAL`.
//!
//! The ordering is the *partial* order of *satisfied-generator inclusion*:
//! a verdict `a` is `≤ b` iff the set of generators `a` satisfies is a
//! *superset* of the set `b` satisfies. Top (`IDEAL`) satisfies every
//! generator; bottom (`SLOP`) satisfies none. Adding a satisfied
//! constraint moves the verdict *down* toward `IDEAL`.
//!
//! The implementation uses an explicit cover relation rather than an
//! integer ordering — the singleton generators are pairwise incomparable,
//! so the Hasse diagram is a 4-cube, not a chain.
//! [`Omega::meet`], [`Omega::join`], [`Omega::implies`], and
//! [`Omega::negation`] are computed generically from the cover, so this
//! engine works for arbitrary finite Heyting algebras — see
//! [`Omega::from_cover_relation`]. Extending `G_qual` from three
//! generators to four required no change to any of them.
//!
//! Categorical / Rust names:
//!
//! | Math             | Rust                                          |
//! |------------------|------------------------------------------------|
//! | `Ω`              | [`Omega`]                                       |
//! | elements of `Ω`  | [`EvaluationValue`]                             |
//! | `⊤`              | [`EvaluationValue::Ideal`] / [`Omega::TOP`]     |
//! | `⊥`              | [`EvaluationValue::Slop`] / [`Omega::BOTTOM`]   |
//! | `χ_S : P → Ω`    | [`crate::core::characteristic_morphism`]        |
//!
//! The top is `IDEAL` — the joint satisfaction of all generators. The
//! bottom is `SLOP`, the unconstrained universe.

use std::collections::{HashMap, HashSet};
use std::fmt;

/// How many quality generators `G_qual` has. The carrier of `Ω` is its
/// powerset, so `|Ω| = 2^GENERATOR_COUNT`.
pub const GENERATOR_COUNT: u32 = 4;

/// The number of elements of `Ω`.
pub const OMEGA_SIZE: usize = 1 << GENERATOR_COUNT;

/// The four quality generators of `G_qual`.
///
/// Re-exported from [`crate::evaluation::preferences`], which is where
/// the *ordering* of generators (operator preference) lives; the
/// generators themselves are part of the definition of `Ω`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Generator {
    Simple,
    Composable,
    Secure,
    Navigable,
}

impl Generator {
    pub const ALL: [Generator; GENERATOR_COUNT as usize] = [
        Generator::Simple,
        Generator::Composable,
        Generator::Secure,
        Generator::Navigable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Generator::Simple => "simple",
            Generator::Composable => "composable",
            Generator::Secure => "secure",
            Generator::Navigable => "navigable",
        }
    }

    /// This generator's bit in the [`EvaluationValue`] encoding — see
    /// that type's doc comment for the canonical statement.
    pub(crate) fn bit(self) -> u8 {
        match self {
            Generator::Simple => 0b0001,
            Generator::Composable => 0b0010,
            Generator::Secure => 0b0100,
            Generator::Navigable => 0b1000,
        }
    }

    /// The singleton verdict this generator produces on its own.
    pub fn value(self) -> EvaluationValue {
        EvaluationValue::from_bits(self.bit()).expect("a single generator bit is a valid verdict")
    }
}

/// The sixteen elements of the free Heyting algebra `H(G_qual)` on four
/// quality generators (`SIMPLE`, `COMPOSABLE`, `SECURE`, `NAVIGABLE`).
///
/// Each value corresponds to the subset of generators a program
/// satisfies. Ordering (via [`Omega::leq`]) is by *superset of satisfied
/// generators*: `a ≤ b` iff every generator satisfied by `b` is also
/// satisfied by `a`. Thus `IDEAL = ⊤` (everything satisfied) and
/// `SLOP = ⊥` (nothing satisfied).
///
/// Encoding (discriminant = bitmask `SIMPLE|COMPOSABLE|SECURE|NAVIGABLE`):
///
/// - bit 0 = `SIMPLE` satisfied
/// - bit 1 = `COMPOSABLE` satisfied
/// - bit 2 = `SECURE` satisfied
/// - bit 3 = `NAVIGABLE` satisfied
///
/// This bit ordering is just the encoding, *not* the lattice order — this
/// type intentionally does not derive `Ord`; use [`Omega::leq`] for the
/// real partial order.
///
/// `IDEAL` remains the top element: before NAVIGABLE it meant "all three
/// generators", now it means "all four". The element at `0b0111` — what
/// `IDEAL` used to name — is now `SIMPLE_COMPOSABLE_SECURE`. That rename
/// is the intended breaking re-grade of v0.5.0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EvaluationValue {
    /// `⊥` — no generator satisfied. The unconstrained universe; total
    /// structural chaos.
    Slop = 0b0000,
    /// Only the `SIMPLE` generator is satisfied.
    Simple = 0b0001,
    /// Only the `COMPOSABLE` generator is satisfied.
    Composable = 0b0010,
    /// Meet of `SIMPLE` and `COMPOSABLE`.
    SimpleComposable = 0b0011,
    /// Only the `SECURE` generator is satisfied.
    Secure = 0b0100,
    /// Meet of `SIMPLE` and `SECURE`.
    SimpleSecure = 0b0101,
    /// Meet of `COMPOSABLE` and `SECURE`.
    ComposableSecure = 0b0110,
    /// Meet of `SIMPLE`, `COMPOSABLE`, and `SECURE` — the top of the
    /// pre-NAVIGABLE algebra, which this element used to be named
    /// `IDEAL`.
    SimpleComposableSecure = 0b0111,
    /// Only the `NAVIGABLE` generator is satisfied.
    Navigable = 0b1000,
    /// Meet of `SIMPLE` and `NAVIGABLE`.
    SimpleNavigable = 0b1001,
    /// Meet of `COMPOSABLE` and `NAVIGABLE`.
    ComposableNavigable = 0b1010,
    /// Meet of `SIMPLE`, `COMPOSABLE`, and `NAVIGABLE`.
    SimpleComposableNavigable = 0b1011,
    /// Meet of `SECURE` and `NAVIGABLE`.
    SecureNavigable = 0b1100,
    /// Meet of `SIMPLE`, `SECURE`, and `NAVIGABLE`.
    SimpleSecureNavigable = 0b1101,
    /// Meet of `COMPOSABLE`, `SECURE`, and `NAVIGABLE`.
    ComposableSecureNavigable = 0b1110,
    /// `⊤` — all four generators satisfied.
    Ideal = 0b1111,
}

impl EvaluationValue {
    /// All sixteen elements, in ascending bitmask order.
    pub const ALL: [EvaluationValue; OMEGA_SIZE] = [
        EvaluationValue::Slop,
        EvaluationValue::Simple,
        EvaluationValue::Composable,
        EvaluationValue::SimpleComposable,
        EvaluationValue::Secure,
        EvaluationValue::SimpleSecure,
        EvaluationValue::ComposableSecure,
        EvaluationValue::SimpleComposableSecure,
        EvaluationValue::Navigable,
        EvaluationValue::SimpleNavigable,
        EvaluationValue::ComposableNavigable,
        EvaluationValue::SimpleComposableNavigable,
        EvaluationValue::SecureNavigable,
        EvaluationValue::SimpleSecureNavigable,
        EvaluationValue::ComposableSecureNavigable,
        EvaluationValue::Ideal,
    ];

    /// The bitmask discriminant
    /// (`SIMPLE=1, COMPOSABLE=2, SECURE=4, NAVIGABLE=8`).
    pub fn bits(self) -> u8 {
        self as u8
    }

    /// How many generators this verdict satisfies. Drives the medal tier.
    pub fn satisfied_count(self) -> u32 {
        self.bits().count_ones()
    }

    /// The Python-enum-style name (`"SIMPLE_COMPOSABLE"`, etc.) — kept
    /// stable across the Rust/Python boundary for any JSON/CLI output
    /// that predates this migration.
    ///
    /// A const lookup table rather than a 16-arm `match`: dogfooding
    /// `topos evaluate` on this file during the migration flagged the
    /// match-statement version for cyclomatic complexity (33, exceeding
    /// the SIMPLE threshold of 15); table lookup by
    /// [`EvaluationValue::bits`] carries the same data with one branch.
    pub fn name(self) -> &'static str {
        const NAMES: [&str; OMEGA_SIZE] = [
            "SLOP",
            "SIMPLE",
            "COMPOSABLE",
            "SIMPLE_COMPOSABLE",
            "SECURE",
            "SIMPLE_SECURE",
            "COMPOSABLE_SECURE",
            "SIMPLE_COMPOSABLE_SECURE",
            "NAVIGABLE",
            "SIMPLE_NAVIGABLE",
            "COMPOSABLE_NAVIGABLE",
            "SIMPLE_COMPOSABLE_NAVIGABLE",
            "SECURE_NAVIGABLE",
            "SIMPLE_SECURE_NAVIGABLE",
            "COMPOSABLE_SECURE_NAVIGABLE",
            "IDEAL",
        ];
        NAMES[self.bits() as usize]
    }

    /// Unicode symbol (medal) for this verdict.
    ///
    /// Derived from [`EvaluationValue::satisfied_count`] rather than a
    /// parallel 16-entry table: the medal *is* the count, so computing it
    /// keeps the two from drifting apart as `G_qual` grows.
    pub fn symbol(self) -> &'static str {
        match self.satisfied_count() {
            4 => "🏆",
            3 => "🥇",
            2 => "🥈",
            1 => "🥉",
            _ => "❌",
        }
    }

    /// Human-readable description of this evaluation value: the medal
    /// tier, then the generators it satisfies.
    pub fn description(self) -> String {
        let tier = match self.satisfied_count() {
            4 => "🏆 PLATINUM - Joint satisfaction of all four quality pillars",
            3 => "🥇 GOLD - Three of four quality pillars satisfied",
            2 => "🥈 SILVER - Two of four quality pillars satisfied",
            1 => "🥉 BRONZE - One of four quality pillars satisfied",
            _ => return "❌ NO MEDAL - Fails every generator; unconstrained code".to_string(),
        };
        format!("{tier} ({})", self.name())
    }

    /// Reconstruct a verdict from its bitmask. `None` if `bits > 0b1111`.
    pub fn from_bits(bits: u8) -> Option<EvaluationValue> {
        EvaluationValue::ALL.into_iter().find(|v| v.bits() == bits)
    }
}

impl fmt::Display for EvaluationValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.symbol(), self.name())
    }
}

/// Map a set of satisfied generators to its free-algebra verdict.
///
/// This is the concrete encoding of the truth table from `README.md`:
/// every subset of `G_qual` is a unique verdict. Duplicates in
/// `satisfied` are harmless — the fold is over set bits.
///
/// Takes a slice rather than one `bool` per generator: positional
/// booleans stop being readable at three and are a live miswiring hazard
/// at four.
pub fn verdict_from_generators(satisfied: &[Generator]) -> EvaluationValue {
    let bits = satisfied.iter().fold(0u8, |acc, g| acc | g.bit());
    EvaluationValue::from_bits(bits)
        .expect("a GENERATOR_COUNT-bit mask is always a valid EvaluationValue")
}

/// Direct cover relation for the default 4-cube: `value -> immediate successors`.
///
/// Each successor *adds* one satisfied generator (turns one bit on),
/// which in this order moves *down* toward `IDEAL`. `cover[a] = [b, ...]`
/// means "`b` is an immediate successor of `a`" (`a` is covered by `b`,
/// `a ≤ b`).
///
/// Generated by flipping each unset bit rather than written out as
/// sixteen literal entries: the Hasse diagram of a powerset lattice *is*
/// single-bit addition, so deriving it is both shorter than the table and
/// correct by construction for any `GENERATOR_COUNT`.
fn default_cover() -> HashMap<EvaluationValue, Vec<EvaluationValue>> {
    EvaluationValue::ALL
        .into_iter()
        .map(|value| {
            let successors = (0..GENERATOR_COUNT)
                .map(|b| 1u8 << b)
                .filter(|bit| value.bits() & bit == 0)
                .filter_map(|bit| EvaluationValue::from_bits(value.bits() | bit))
                .collect();
            (value, successors)
        })
        .collect()
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
    /// The default 4-cube lattice on
    /// `{SIMPLE, COMPOSABLE, SECURE, NAVIGABLE}`.
    fn default() -> Self {
        Omega::from_cover_relation(default_cover())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The singleton verdicts — the atoms of the 4-cube.
    fn atoms() -> Vec<EvaluationValue> {
        Generator::ALL.into_iter().map(|g| g.value()).collect()
    }

    #[test]
    fn evaluation_value_order() {
        let lattice = Omega::default();
        for atom in atoms() {
            assert!(lattice.leq(EvaluationValue::Slop, atom));
            assert!(lattice.leq(atom, EvaluationValue::Ideal));
        }

        // Every pair of atoms is incomparable — that is what makes the
        // order genuinely partial rather than a chain.
        for a in atoms() {
            for b in atoms() {
                if a != b {
                    assert!(!lattice.leq(a, b), "{a} and {b} must be incomparable");
                }
            }
        }

        assert!(lattice.leq(EvaluationValue::Simple, EvaluationValue::SimpleComposable));
        assert!(lattice.leq(EvaluationValue::SimpleComposable, EvaluationValue::Ideal));
        assert!(!lattice.leq(EvaluationValue::SimpleComposable, EvaluationValue::Secure));
        // The NAVIGABLE axis is a real dimension, not a relabelling: the
        // old top sits strictly below the new one.
        assert!(lattice.leq(
            EvaluationValue::SimpleComposableSecure,
            EvaluationValue::Ideal
        ));
        assert!(!lattice.leq(
            EvaluationValue::Ideal,
            EvaluationValue::SimpleComposableSecure
        ));
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
            Ok(EvaluationValue::SimpleComposableSecure)
        );
        assert_eq!(
            lattice.join(
                EvaluationValue::SimpleComposableSecure,
                EvaluationValue::Navigable
            ),
            Ok(EvaluationValue::Ideal)
        );
        assert_eq!(
            lattice.join(EvaluationValue::Slop, EvaluationValue::Simple),
            Ok(EvaluationValue::Simple)
        );
    }

    /// Meet and join must agree with set intersection and union of the
    /// satisfied generators, on every one of the 256 pairs — the property
    /// that makes this a powerset lattice rather than an arbitrary poset.
    #[test]
    fn meet_and_join_are_set_operations_everywhere() {
        let lattice = Omega::default();
        for a in EvaluationValue::ALL {
            for b in EvaluationValue::ALL {
                assert_eq!(
                    lattice.meet(a, b),
                    Ok(EvaluationValue::from_bits(a.bits() & b.bits()).unwrap()),
                    "meet({a}, {b})"
                );
                assert_eq!(
                    lattice.join(a, b),
                    Ok(EvaluationValue::from_bits(a.bits() | b.bits()).unwrap()),
                    "join({a}, {b})"
                );
            }
        }
    }

    /// The defining adjunction of a Heyting algebra: `a ≤ (b → c)` iff
    /// `a ∧ b ≤ c`. Checked over all 4096 triples, so the 4-cube is
    /// verified to still be a Heyting algebra rather than assumed to be.
    #[test]
    fn implication_is_right_adjoint_to_meet() {
        let lattice = Omega::default();
        for a in EvaluationValue::ALL {
            for b in EvaluationValue::ALL {
                let implication = lattice.implies(b, a).expect("implication exists");
                for c in EvaluationValue::ALL {
                    let meet = lattice.meet(c, b).expect("meet exists");
                    assert_eq!(
                        lattice.leq(c, implication),
                        lattice.leq(meet, a),
                        "adjunction failed at a={a}, b={b}, c={c}"
                    );
                }
            }
        }
    }

    #[test]
    fn verdict_from_generators_truth_table() {
        // Exhaustive over all 16 subsets of G_qual: each subset must map
        // to the unique verdict whose bitmask it is.
        for value in EvaluationValue::ALL {
            let satisfied: Vec<Generator> = Generator::ALL
                .into_iter()
                .filter(|g| value.bits() & g.bit() != 0)
                .collect();
            assert_eq!(verdict_from_generators(&satisfied), value);
        }
        assert_eq!(verdict_from_generators(&[]), EvaluationValue::Slop);
        assert_eq!(
            verdict_from_generators(&Generator::ALL),
            EvaluationValue::Ideal
        );
    }

    #[test]
    fn evaluation_value_properties() {
        assert_eq!(EvaluationValue::Ideal.symbol(), "🏆");
        assert_eq!(EvaluationValue::SimpleComposableSecure.symbol(), "🥇");
        assert_eq!(EvaluationValue::SimpleComposable.symbol(), "🥈");
        assert_eq!(EvaluationValue::Slop.symbol(), "❌");
        for atom in atoms() {
            assert_eq!(atom.symbol(), "🥉");
        }
        assert!(EvaluationValue::Ideal
            .description()
            .to_lowercase()
            .contains("platinum"));
        assert!(EvaluationValue::SimpleComposableSecure
            .description()
            .to_lowercase()
            .contains("gold"));
        assert!(EvaluationValue::Composable
            .description()
            .to_lowercase()
            .contains("composable"));
    }

    /// The medal tier is the count of satisfied pillars, and nothing else.
    #[test]
    fn medal_tier_tracks_satisfied_count() {
        for value in EvaluationValue::ALL {
            let expected = match value.satisfied_count() {
                4 => "🏆",
                3 => "🥇",
                2 => "🥈",
                1 => "🥉",
                _ => "❌",
            };
            assert_eq!(value.symbol(), expected, "{value}");
        }
    }

    /// Names must be unique and must spell out the generators they
    /// satisfy, so an agent reading a verdict string can act on it.
    #[test]
    fn names_are_unique_and_spell_out_their_generators() {
        let names: HashSet<&str> = EvaluationValue::ALL.iter().map(|v| v.name()).collect();
        assert_eq!(names.len(), OMEGA_SIZE);
        for value in EvaluationValue::ALL {
            if matches!(value, EvaluationValue::Slop | EvaluationValue::Ideal) {
                continue;
            }
            for generator in Generator::ALL {
                let mentioned = value.name().to_lowercase().contains(generator.as_str());
                assert_eq!(
                    mentioned,
                    value.bits() & generator.bit() != 0,
                    "{value} vs {}",
                    generator.as_str()
                );
            }
        }
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
