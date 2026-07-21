//! Clone detection policy (pairwise, outside `Ω`).
//!
//! Functors compute normalized AST distance; this module applies the
//! clone threshold. Not a `Φᵢ` translator — does not participate in the
//! SIMPLE / COMPOSABLE / SECURE lattice. Default lives in
//! [`crate::evaluation::policies::calibration`].

use crate::core::object::ProgramObject;
use crate::evaluation::policies::calibration::CLONE;
use crate::functors::profunctors::ast::compare::calculate_ast_distance;

/// Check if two programs are structural clones.
///
/// Programs are considered clones if their normalized distance is
/// below `threshold`.
pub fn are_clones(source: &ProgramObject, target: &ProgramObject, threshold: f64) -> bool {
    calculate_ast_distance(source, target).normalized_distance <= threshold
}

/// [`are_clones`] using [`CLONE::max_normalized_distance`] as the
/// default threshold, matching the Python original's keyword default.
pub fn are_clones_default(source: &ProgramObject, target: &ProgramObject) -> bool {
    are_clones(source, target, CLONE.max_normalized_distance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphs::ast::dispatch::parse_source;

    fn build(source: &str) -> ProgramObject {
        let result = parse_source(source, "python", None).expect("parse should not fail");
        ProgramObject::new(
            result.tree,
            result.source,
            result.language,
            result.uast_root,
        )
    }

    #[test]
    fn identical_sources_are_clones() {
        let a = build("def f(x):\n    return x + 1\n");
        let b = build("def f(x):\n    return x + 1\n");
        assert!(are_clones_default(&a, &b));
    }

    #[test]
    fn structurally_different_sources_are_not_clones() {
        let a = build("x = 1\n");
        let b = build(
            "class Foo:\n    def bar(self, x, y):\n        for i in range(x):\n            if i % 2 == 0:\n                yield i * y\n",
        );
        assert!(!are_clones_default(&a, &b));
    }

    #[test]
    fn custom_threshold_is_respected() {
        let a = build("x = 1");
        let b = build("y = 2");
        let dist = calculate_ast_distance(&a, &b).normalized_distance;
        assert!(are_clones(&a, &b, dist));
        assert!(!are_clones(&a, &b, dist - 0.001));
    }
}
