"""SECURE diagnostics surfaced by MCP tools."""

from __future__ import annotations

import shutil
from typing import Any

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

    If the ``sighthound`` CLI is on ``PATH``, findings come from its ruleset
    (via :mod:`topos.utils.sighthound`); otherwise local CPG probes are used.
    """
    if cpg is None:
        return []

    if shutil.which("sighthound"):
        return _sighthound_security_findings(
            cpg, max_findings=max_findings, allow=allow
        )

    findings = dangerous_call_findings(cpg, max_findings=max_findings, allow=allow)
    remaining = max(0, max_findings - len(findings))
    if remaining:
        findings.extend(
            taint_flow_findings(cpg, max_findings=remaining, allow=allow)
        )
    return findings


def _sighthound_security_findings(
    cpg: CodePropertyGraph,
    *,
    max_findings: int,
    allow: set[str] | None,
) -> list[SecurityFinding]:
    """Map Sighthound JSON findings into :class:`SecurityFinding` models."""
    from topos.utils.sighthound import (
        finding_callee,
        finding_line,
        finding_sink_text,
        finding_snippet,
        finding_source_text,
        is_taint_finding,
        run_sighthound_scan,
    )

    file_path = None
    for node in cpg.nodes.values():
        if node.uast and node.uast.span and node.uast.span.file:
            file_path = node.uast.span.file
            break

    raw_findings = run_sighthound_scan(cpg.source, cpg.language, file_path)
    registry = (
        effective_registry(cpg.language, allow) if allow is not None else None
    )

    findings: list[SecurityFinding] = []
    for raw in raw_findings:
        if not isinstance(raw, dict):
            continue
        mapped = _map_sighthound_finding(
            raw,
            registry=registry,
            finding_callee=finding_callee,
            finding_line=finding_line,
            finding_sink_text=finding_sink_text,
            finding_snippet=finding_snippet,
            finding_source_text=finding_source_text,
            is_taint_finding=is_taint_finding,
        )
        if mapped is None:
            continue
        findings.append(mapped)
        if len(findings) >= max_findings:
            break
    return findings


def _map_sighthound_finding(
    raw: dict[str, Any],
    *,
    registry: set[str] | None,
    finding_callee: Any,
    finding_line: Any,
    finding_sink_text: Any,
    finding_snippet: Any,
    finding_source_text: Any,
    is_taint_finding: Any,
) -> SecurityFinding | None:
    """Convert one Sighthound finding; return None if allowlisted away."""
    callee = finding_callee(raw)
    # When an allow set is active, ``effective_registry`` already dropped
    # allowlisted callees. Skip findings whose callee is no longer dangerous.
    if registry is not None and callee and not _matches_registry(callee, registry):
        return None

    taint = is_taint_finding(raw)
    kind = "taint_flow" if taint else "dangerous_call"
    source = finding_source_text(raw) if taint else None
    sink = finding_sink_text(raw) if taint else None

    return SecurityFinding(
        kind=kind,
        line=finding_line(raw),
        snippet=finding_snippet(raw),
        callee=callee,
        source=source,
        sink=sink,
    )


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


def _build_forward_ddg_map(cpg: CodePropertyGraph) -> dict[str, list[str]]:
    forward: dict[str, list[str]] = {}
    for edge in cpg.edges:
        if edge.kind is CPGEdgeKind.DDG:
            forward.setdefault(edge.source, []).append(edge.target)
    return forward


def _find_sources_and_sinks(
    cpg: CodePropertyGraph, source_registry: set[str], sink_registry: set[str]
) -> tuple[list[str], dict[str, str]]:
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
    return sources, sinks


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

    forward = _build_forward_ddg_map(cpg)
    if not forward:
        return []

    sources, sinks = _find_sources_and_sinks(cpg, source_registry, sink_registry)
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
