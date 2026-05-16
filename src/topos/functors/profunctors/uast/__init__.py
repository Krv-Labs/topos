"""UAST profunctors — cross-language structural comparison."""

from topos.functors.profunctors.uast.compare import (
    UASTComparison,
    compare_uast,
    uast_edit_distance,
    uast_kind_distance,
)
from topos.functors.profunctors.uast.structural_test_coverage import (
    DeclarationCoverageReport,
    declaration_coverage,
)

__all__ = [
    "UASTComparison",
    "compare_uast",
    "uast_edit_distance",
    "uast_kind_distance",
    "DeclarationCoverageReport",
    "declaration_coverage",
]
