//! UAST profunctors — cross-language structural comparison.

pub mod compare;
pub mod structural_test_coverage;

pub use compare::{compare_uast, uast_edit_distance, uast_kind_distance, UASTComparison};
pub use structural_test_coverage::{declaration_coverage, DeclarationCoverageReport};
