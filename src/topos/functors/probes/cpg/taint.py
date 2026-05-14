"""
Taint-flow probe (CPG → ℝ).

A *taint flow* is a source → sink data-flow path along DDG edges in the
CPG, optionally interrupted by a sanitizer.  v1 implements a purely
syntactic version: we mark every input-like identifier as a *source* and
every dangerous-API call site as a *sink*; the probe counts DDG paths
between them.

Per-language source / sink registries are intentionally tiny — refine
when real applications surface false negatives.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from topos.functors.probes.cpg.danger import (
    DANGEROUS_APIS,
    _callee_from_text,
    _matches_registry,
)

if TYPE_CHECKING:
    from topos.graphs.cpg.object import CodePropertyGraph

# Names whose value should be treated as untrusted input.
TAINT_SOURCES: dict[str, set[str]] = {
    "python": {
        "input",
        "sys.argv",
        "request.args",
        "request.form",
        "request.json",
        "os.environ",
    },
    "javascript": {
        "process.argv",
        "process.env",
        "req.body",
        "req.query",
        "document.location",
        "window.location",
    },
    "typescript": {
        "process.argv",
        "process.env",
        "req.body",
        "req.query",
    },
    "rust": {
        "std::env::args",
        "std::env::var",
    },
    "cpp": {
        "argv",
        "getenv",
        "scanf",
    },
}


def taint_flow_paths(cpg: CodePropertyGraph) -> int:
    """
    Count DDG paths from any taint source to any dangerous-API sink.

    A DDG path here is a chain of CPG nodes connected by DDG edges; we
    count distinct ``(source_node, sink_node)`` pairs that are reachable.
    """
    source_registry = TAINT_SOURCES.get(cpg.language, set())
    sink_registry = DANGEROUS_APIS.get(cpg.language, set())
    if not source_registry or not sink_registry:
        return 0

    # Build DDG adjacency (forward).
    from topos.graphs.cpg.models import CPGEdgeKind

    forward: dict[str, list[str]] = {}
    for edge in cpg.edges:
        if edge.kind is not CPGEdgeKind.DDG:
            continue
        forward.setdefault(edge.source, []).append(edge.target)

    if not forward:
        return 0

    sources: list[str] = []
    sinks: set[str] = set()
    for nid, node in cpg.nodes.items():
        text = cpg.node_text(node)
        if not text:
            continue
        snippet = text.strip()
        if node.kind == "CallExpr":
            callee = _callee_from_text(snippet)
            if callee and _matches_registry(callee, sink_registry):
                sinks.add(nid)
        if node.kind == "Identifier" and snippet in source_registry:
            sources.append(nid)
        elif node.kind == "MemberExpr" and snippet in source_registry:
            sources.append(nid)

    if not sources or not sinks:
        return 0

    total = 0
    for src in sources:
        reachable = _bfs_reachable(forward, src)
        total += sum(1 for s in sinks if s in reachable)
    return total


def _bfs_reachable(adj: dict[str, list[str]], start: str) -> set[str]:
    visited: set[str] = {start}
    frontier = [start]
    while frontier:
        node = frontier.pop()
        for nxt in adj.get(node, []):
            if nxt in visited:
                continue
            visited.add(nxt)
            frontier.append(nxt)
    return visited
