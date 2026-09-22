//! UAST profunctors — cross-language structural comparison.

pub mod compare;
pub mod ledger;
pub mod structural_test_coverage;

pub use compare::{compare_uast, uast_edit_distance, uast_kind_distance, UASTComparison};
pub use ledger::{
    match_functions, snapshot_functions, FunctionMatch, FunctionSnapshot, Ledger, LedgerTotals,
    MatchKind, ANONYMOUS_NAME, MOVED_MODIFIED_MIN_SIMILARITY, NAME_MATCH_MIN_SIMILARITY,
};
pub use structural_test_coverage::{declaration_coverage, DeclarationCoverageReport};
