"""
UAST Metrics Module
-------------------
Cross-language structural metrics built on UAST `kind` values.
"""

from topos.functors.probes.uast.compare import (
    UASTComparison,
    compare_uast,
    uast_edit_distance,
    uast_kind_distance,
)
from topos.functors.probes.uast.signature import (
    CONTROL_FLOW_KINDS,
    StructuralSummary,
    control_flow_profile,
    structural_summary,
    uast_dfs_kind_sequence,
    uast_kind_histogram,
)
from topos.functors.probes.uast.structural_test_coverage import (
    DeclarationCoverageReport,
    StructuralTestCoverageReport,
    declaration_coverage,
    extract_declarations,
    merge_control_flow_profiles,
    merge_kgram_counters,
    merge_uast_kind_histograms,
    structural_test_coverage,
)

__all__ = [
    "CONTROL_FLOW_KINDS",
    "DeclarationCoverageReport",
    "StructuralSummary",
    "StructuralTestCoverageReport",
    "UASTComparison",
    "compare_uast",
    "control_flow_profile",
    "declaration_coverage",
    "extract_declarations",
    "merge_control_flow_profiles",
    "merge_kgram_counters",
    "merge_uast_kind_histograms",
    "structural_summary",
    "structural_test_coverage",
    "uast_dfs_kind_sequence",
    "uast_edit_distance",
    "uast_kind_distance",
    "uast_kind_histogram",
]
