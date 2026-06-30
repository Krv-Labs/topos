"""
Process-flow probes (ProcessFlow list -> ℝ).

These measure GitNexus execution flows that touch a target file. They are the
interprocedural counterpart to the intra-procedural CFG/CPG probes: where the
CFG sees one function and the CPG sees one file, a *process* is a reconstructed
call sequence that can span many functions and files.

    SIMPLE      max_flow_length, flow_participation
    COMPOSABLE  max_community_span, cross_community_flow_count
    SECURE      dangerous_flow_count   (sink-on-flow reachability)
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from topos.functors.probes.cpg.danger import DANGEROUS_APIS, _matches_registry

if TYPE_CHECKING:
    from topos.graphs.process.object import ProcessFlow


def max_flow_length(flows: list[ProcessFlow]) -> int:
    """Longest flow (number of steps) among the given flows; 0 if none."""
    return max((f.step_count for f in flows), default=0)


def flow_participation(flows: list[ProcessFlow]) -> int:
    """Number of distinct flows the file participates in."""
    return len(flows)


def max_community_span(flows: list[ProcessFlow]) -> int:
    """Most community boundaries crossed by any single flow; 0 if none."""
    return max((len(f.communities) for f in flows), default=0)


def cross_community_flow_count(flows: list[ProcessFlow]) -> int:
    """Count of flows GitNexus classified as crossing community boundaries."""
    return sum(1 for f in flows if f.process_type == "cross_community")


def dangerous_flow_count(flows: list[ProcessFlow], language: str) -> int:
    """
    Count flows that contain at least one dangerous-API step.

    A *dangerous step* is a flow step whose symbol name matches the
    per-language dangerous-API registry (shared with the intra-file CPG danger
    probe). Unlike that probe, this counts reachability along a reconstructed
    interprocedural execution flow, not call sites within a single file.
    """
    registry = DANGEROUS_APIS.get(language, set())
    if not registry:
        return 0
    return sum(1 for f in flows if _flow_has_dangerous_step(f, registry))


def _flow_has_dangerous_step(flow: ProcessFlow, registry: set[str]) -> bool:
    return any(
        name and _matches_registry(name, registry) for name in flow.step_names()
    )
