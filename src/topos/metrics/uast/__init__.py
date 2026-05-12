"""
UAST Metrics Module
-------------------
Cross-language structural metrics built on UAST `kind` values.
"""

from topos.metrics.uast.compare import (
    UASTComparison,
    compare_uast,
    uast_edit_distance,
    uast_kind_distance,
)
from topos.metrics.uast.signature import (
    CONTROL_FLOW_KINDS,
    StructuralSummary,
    control_flow_profile,
    structural_summary,
    uast_kind_histogram,
)

__all__ = [
    "CONTROL_FLOW_KINDS",
    "StructuralSummary",
    "UASTComparison",
    "compare_uast",
    "control_flow_profile",
    "structural_summary",
    "uast_edit_distance",
    "uast_kind_distance",
    "uast_kind_histogram",
]
