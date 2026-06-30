"""
Process-flow probes.

Pure measurements over GitNexus execution flows (``ProcessFlow``):
interprocedural flow length / participation (SIMPLE), community span and
cross-community flow counts (COMPOSABLE), and dangerous-step reachability
(SECURE). See :mod:`topos.graphs.process.object` for the representation that
calls these.
"""

from topos.functors.probes.process.flow import (
    cross_community_flow_count,
    dangerous_flow_count,
    flow_participation,
    max_community_span,
    max_flow_length,
)

__all__ = [
    "max_flow_length",
    "flow_participation",
    "max_community_span",
    "cross_community_flow_count",
    "dangerous_flow_count",
]
