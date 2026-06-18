"""SECURE diagnostics surfaced by MCP tools."""

from __future__ import annotations

from topos.functors.probes.cpg.danger import (
    _callee_from_text,
    _matches_registry,
    effective_registry,
)
from topos.functors.probes.cpg.taint import TAINT_SOURCES
from topos.graphs.cpg.models import CPGEdgeKind
from topos.graphs.cpg.object import CodePropertyGraph

from .schemas import SecurityFinding


def security_findings(
    cpg: CodePropertyGraph | None,
    *,
    max_findings: int = 20,
    allow: set[str] | None = None,
) -> list[SecurityFinding]:
    """Return concise dangerous-call and taint-flow diagnostics.

    When *allow* is given, allowlisted patterns are excluded from the
    registry first.  ``allow=None`` preserves canonical behavior.
    """
    if cpg is None:
        return []
    findings = dangerous_call_findings(cpg, max_findings=max_findings, allow=allow)
    remaining = max(0, max_findings - len(findings))
    if remaining:
        findings.extend(
            taint_flow_findings(cpg, max_findings=remaining, allow=allow)
        )
    return findings


def dangerous_call_findings(
    cpg: CodePropertyGraph,
    *,
    max_findings: int = 20,
    allow: set[str] | None = None,
) -> list[SecurityFinding]:
    """Find dangerous API call sites with source locations."""
    registry = effective_registry(cpg.language, allow)
    if not registry:
        return []

    findings: list[SecurityFinding] = []
    for node in cpg.nodes.values():
        if node.kind != "CallExpr":
            continue
        text = cpg.node_text(node).strip()
        if not text:
            continue
        callee = _callee_from_text(text)
        if not callee or not _matches_registry(callee, registry):
            continue
        findings.append(
            SecurityFinding(
                kind="dangerous_call",
                line=max(1, node.uast.span.start_line),
                snippet=_line_snippet(cpg.source, node.uast.span.start_line) or text,
                callee=callee,
            )
        )
        if len(findings) >= max_findings:
            break
    return findings


def taint_flow_findings(
    cpg: CodePropertyGraph,
    *,
    max_findings: int = 20,
    allow: set[str] | None = None,
) -> list[SecurityFinding]:
    """Find source-to-dangerous-sink DDG paths with source/sink snippets."""
    source_registry = TAINT_SOURCES.get(cpg.language, set())
    sink_registry = effective_registry(cpg.language, allow)
    if not source_registry or not sink_registry:
        return []

    forward: dict[str, list[str]] = {}
    for edge in cpg.edges:
        if edge.kind is CPGEdgeKind.DDG:
            forward.setdefault(edge.source, []).append(edge.target)
    if not forward:
        return []

    sources: list[str] = []
    sinks: dict[str, str] = {}
    for node_id, node in cpg.nodes.items():
        snippet = cpg.node_text(node).strip()
        if not snippet:
            continue
        if node.kind == "CallExpr":
            callee = _callee_from_text(snippet)
            if callee and _matches_registry(callee, sink_registry):
                sinks[node_id] = snippet
        if node.kind in ("Identifier", "MemberExpr") and snippet in source_registry:
            sources.append(node_id)

    findings: list[SecurityFinding] = []
    for source_id in sources:
        reachable = _bfs_reachable(forward, source_id)
        for sink_id, sink_snippet in sinks.items():
            if sink_id not in reachable:
                continue
            source_node = cpg.nodes[source_id]
            sink_node = cpg.nodes[sink_id]
            source_snippet = cpg.node_text(source_node).strip()
            findings.append(
                SecurityFinding(
                    kind="taint_flow",
                    line=max(1, sink_node.uast.span.start_line),
                    snippet=(
                        _line_snippet(cpg.source, sink_node.uast.span.start_line)
                        or sink_snippet
                    ),
                    source=source_snippet,
                    sink=sink_snippet,
                    callee=_callee_from_text(sink_snippet) or None,
                )
            )
            if len(findings) >= max_findings:
                return findings
    return findings


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


def _line_snippet(source: str, line: int) -> str:
    if line <= 0:
        return ""
    lines = source.splitlines()
    if line > len(lines):
        return ""
    return lines[line - 1].strip()
