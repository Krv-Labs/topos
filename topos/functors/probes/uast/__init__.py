"""
UAST probes — single-program structural signatures over UAST kinds.

Pairwise comparison and structural test-coverage (two-program operations)
live in :mod:`topos.functors.profunctors.uast`; this package holds only
the per-program probes ``P : E → ℝ`` that those profunctors compose.
"""

from topos.functors.probes.uast.abstractness import calculate_abstractness
from topos.functors.probes.uast.signature import (
    CONTROL_FLOW_KINDS,
    StructuralSummary,
    control_flow_profile,
    structural_summary,
    uast_dfs_kind_sequence,
    uast_kind_histogram,
)

__all__ = [
    "CONTROL_FLOW_KINDS",
    "StructuralSummary",
    "calculate_abstractness",
    "control_flow_profile",
    "structural_summary",
    "uast_dfs_kind_sequence",
    "uast_kind_histogram",
]
