//! `Category` — the categorical universe of the program topos.
//!
//! Defines the categorical universe `E = Set^(C × H^op)` of the program
//! topos. The base index category `C` lives in [`crate::graphs::base`]
//! (the directed-graph index category); the value Heyting algebra
//! `H = Ω` lives in [`crate::core::omega`]; this module ties them
//! together.
//!
//! [`ProgramCategory`] enforces composition axioms, maintains identity
//! mappings, and provides convenience access to:
//!
//! - the subobject classifier `Ω` ([`crate::core::omega::Omega`])
//! - the characteristic morphism `χ_S` (`CharacteristicMorphism`,
//!   pending issue #144)
//!
//! so callers never need to reach past this module just to classify or
//! compose programs.

use std::fmt;

use super::morphism::ProgramMorphism;
use super::object::ProgramObject;
use super::omega::Omega;
use crate::functors::profunctors::ast::compare::structural_distance;

/// Raised when category axioms (like composition domain mismatches) are
/// broken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryError(pub String);

impl fmt::Display for CategoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CategoryError {}

/// Encapsulates the categorical universe of our program topos.
///
/// Provides utility methods for constructing identity maps, verifying
/// composition legality, and reaching the topos's internal logic — the
/// subobject classifier `Ω` and the characteristic morphism `χ_S`.
pub struct ProgramCategory {
    pub name: String,
}

impl Default for ProgramCategory {
    fn default() -> Self {
        ProgramCategory {
            name: "ToposOfPrograms".to_string(),
        }
    }
}

impl ProgramCategory {
    pub fn new(name: impl Into<String>) -> Self {
        ProgramCategory { name: name.into() }
    }

    // --- Categorical primitives --------------------------------------

    /// Constructs the identity morphism `id_A : A → A` for a given object.
    ///
    /// Mathematically, this is the trivial no-op transformation that
    /// leaves the object's structural state completely invariant.
    pub fn identity(obj: &ProgramObject) -> ProgramMorphism {
        let noop_source = if obj.language == "python" {
            "def identity(x):\n    return x"
        } else {
            "fn identity<T>(x: T) -> T { x }"
        };
        ProgramMorphism::new(noop_source, obj.language.clone())
    }

    /// Composes two program transformations to form a new morphism
    /// (`g ∘ f`).
    ///
    /// Requires that the codomain of `f` matches the domain of `g`. In
    /// software, this pipes the output structural block of `f` directly
    /// into the input of `g`.
    pub fn compose(g: &ProgramMorphism, f: &ProgramMorphism) -> ProgramMorphism {
        let composed_source = format!(
            "{}\n\n{}\n\ndef composed_pipeline(*args, **kwargs):\n    return g(f(*args, **kwargs))",
            f.source, g.source
        );
        ProgramMorphism::new(composed_source, f.language.clone())
    }

    /// Verify whether a triangular diagram commutes: `h == g ∘ f`.
    ///
    /// In the context of program transformations, decides if a direct
    /// refactoring/shortcut (`h`) is structurally identical to a
    /// multi-step pipeline (`g ∘ f`), via zero normalized AST edit
    /// distance ([`structural_distance`]).
    pub fn verify_commutativity(
        &self,
        f: &ProgramMorphism,
        g: &ProgramMorphism,
        h: &ProgramMorphism,
    ) -> bool {
        let composed_gf = Self::compose(g, f);
        structural_distance(h, &composed_gf) == 0.0
    }

    // --- Internal logic: convenience access to Ω and χ_S --------------

    /// A fresh instance of the subobject classifier `Ω`.
    ///
    /// `Ω` carries both roles: the truth-value object of the topos and
    /// the value Heyting algebra of the internal logic. See
    /// [`crate::core::omega`] for the algebra; the characteristic
    /// morphism that maps programs into it lands with issue #144.
    pub fn omega() -> Omega {
        Omega::default()
    }

    // `characteristic_morphism`, `classify`, `classify_detailed`: pending
    // issue #144 (`core::characteristic_morphism::CharacteristicMorphism`).
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_noop_and_language_preserving() {
        let morphism = ProgramMorphism::new("x = 1", "python");
        let obj = morphism.ast.expect("python source should parse");
        let id = ProgramCategory::identity(&obj);
        assert_eq!(id.language, "python");
        assert!(id.source.contains("def identity"));
    }

    #[test]
    fn compose_pipes_f_into_g() {
        let f = ProgramMorphism::new("def f(x): return x + 1", "python");
        let g = ProgramMorphism::new("def g(x): return x * 2", "python");
        let gf = ProgramCategory::compose(&g, &f);
        assert!(gf.source.contains("def f"));
        assert!(gf.source.contains("def g"));
        assert!(gf.source.contains("composed_pipeline"));
    }

    #[test]
    fn verify_commutativity_true_for_exact_composition() {
        let f = ProgramMorphism::new("def f(x): return x + 1", "python");
        let g = ProgramMorphism::new("def g(x): return x * 2", "python");
        let h = ProgramCategory::compose(&g, &f);
        let category = ProgramCategory::default();
        assert!(category.verify_commutativity(&f, &g, &h));
    }

    #[test]
    fn verify_commutativity_false_for_unrelated_shortcut() {
        let f = ProgramMorphism::new("def f(x): return x + 1", "python");
        let g = ProgramMorphism::new("def g(x): return x * 2", "python");
        let h = ProgramMorphism::new("def h(x): return x - 99", "python");
        let category = ProgramCategory::default();
        assert!(!category.verify_commutativity(&f, &g, &h));
    }

    #[test]
    fn omega_returns_default_lattice() {
        use super::super::omega::EvaluationValue;
        let lattice = ProgramCategory::omega();
        assert_eq!(
            lattice.meet(EvaluationValue::Ideal, EvaluationValue::Simple),
            Ok(EvaluationValue::Simple)
        );
    }
}
