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
    _callee_from_text,
    _matches_registry,
    effective_registry,
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
    "go": {
        "os.Getenv",
        "os.Args",
        "r.FormValue",
        "r.URL",
        "flag.String",
    },
}


def taint_flow_paths(cpg: CodePropertyGraph, allow: set[str] | None = None) -> int:
    """
    Count DDG paths from any taint source to any dangerous-API sink.

    A DDG path here is a chain of CPG nodes connected by DDG edges; we
    count distinct ``(source_node, sink_node)`` pairs that are reachable.

    DDG edges connect whole *statement* nodes (see
    ``ProgramDependenceGraph._compute_data_dependence``), while sources and
    sinks are detected at the finer *sub-expression* granularity (a bare
    ``Identifier``/``MemberExpr`` for sources, a ``CallExpr`` for sinks).
    These two id spaces essentially never intersect directly, so before
    running BFS we bridge the gap by containment: each source/sink is
    mapped to the smallest DDG-participating statement whose byte span
    contains it (falling back to itself when it already *is* a
    DDG-participating statement, keeping the flat/simple case working
    unchanged).

    When *allow* is given, allowlisted sink patterns are excluded; the
    default ``allow=None`` preserves the canonical metrics behavior.
    """
    source_registry = TAINT_SOURCES.get(cpg.language, set())
    sink_registry = effective_registry(cpg.language, allow)
    if not source_registry or not sink_registry:
        return 0

    # Build DDG adjacency (forward).
    from topos.graphs.cpg.models import CPGEdgeKind

    forward: dict[str, list[str]] = {}
    ddg_stmt_ids: set[str] = set()
    for edge in cpg.edges:
        if edge.kind is not CPGEdgeKind.DDG:
            continue
        forward.setdefault(edge.source, []).append(edge.target)
        ddg_stmt_ids.add(edge.source)
        ddg_stmt_ids.add(edge.target)

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
        if node.kind in ("Identifier", "MemberExpr") and snippet in source_registry:
            sources.append(nid)

    if not sources or not sinks:
        return 0

    # {ddg_participating_stmt_id: (start_byte, end_byte)} — built once up
    # front so resolving each source/sink's enclosing statement is a single
    # pass over this (typically small) set, not a scan of every CPG node.
    ddg_spans: dict[str, tuple[int, int]] = {}
    for stmt_id in ddg_stmt_ids:
        stmt_node = cpg.nodes.get(stmt_id)
        if stmt_node is not None:
            ddg_spans[stmt_id] = (
                stmt_node.uast.span.start_byte,
                stmt_node.uast.span.end_byte,
            )

    effective_sources = _resolve_effective_ids(sources, cpg, ddg_spans)
    effective_sinks = _resolve_effective_ids(sinks, cpg, ddg_spans)

    if not effective_sources or not effective_sinks:
        return 0

    total = 0
    reachable_cache: dict[str, set[str]] = {}
    for src in sources:
        eff_src = effective_sources.get(src)
        if eff_src is None:
            continue
        reachable = reachable_cache.get(eff_src)
        if reachable is None:
            reachable = _bfs_reachable(forward, eff_src)
            reachable_cache[eff_src] = reachable
        for sink in sinks:
            eff_sink = effective_sinks.get(sink)
            if eff_sink is not None and eff_sink in reachable:
                total += 1
    return total


def _resolve_effective_ids(
    candidate_ids: list[str] | set[str],
    cpg: CodePropertyGraph,
    ddg_spans: dict[str, tuple[int, int]],
) -> dict[str, str]:
    """
    Map each source/sink node id to its "effective" DDG-graph entry point.

    A candidate that already equals a DDG-participating statement id maps
    to itself (the flat/simple case that already worked before this fix).
    Otherwise we find the smallest DDG-participating statement span that
    contains the candidate's span — its nearest enclosing statement — since
    that's the node the DDG adjacency actually knows how to traverse from.
    Ties (equal-width enclosing spans) keep the first one encountered;
    candidates with no enclosing DDG statement (e.g. dead code sliced out
    of the CFG) are simply omitted from the result.
    """
    resolved: dict[str, str] = {}
    for nid in candidate_ids:
        if nid in ddg_spans:
            resolved[nid] = nid
            continue
        node = cpg.nodes.get(nid)
        if node is None:
            continue
        start, end = node.uast.span.start_byte, node.uast.span.end_byte
        best_id: str | None = None
        best_width: int | None = None
        for stmt_id, (s_start, s_end) in ddg_spans.items():
            if s_start <= start and end <= s_end:
                width = s_end - s_start
                if best_width is None or width < best_width:
                    best_width = width
                    best_id = stmt_id
        if best_id is not None:
            resolved[nid] = best_id
    return resolved


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
