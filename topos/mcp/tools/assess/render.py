"""
Markdown rendering and diff helpers for assessment tools.
"""

from __future__ import annotations

import difflib

from topos.core.morphism import ProgramMorphism
from topos.functors.probes.cfg.complexity import cyclomatic_complexity
from topos.graphs.cfg.builder import _collect_callables, build_cfg_from_uast
from topos.graphs.cfg.object import ControlFlowGraph

from ...schemas import AssessmentResult, AssessmentStatus

# Statuses that warrant a targeted regression diff.
_REGRESSION_STATUSES = frozenset(
    {
        AssessmentStatus.REGRESSION,
        AssessmentStatus.REGRESSION_SCORE,
        AssessmentStatus.SUSPICIOUS_NO_STRUCTURAL_CHANGE,
    }
)

_REGRESSION_DIFF_MAX_LINES = 40


def _span_text(source_bytes: bytes, span) -> str:
    """Slice a UAST byte span out of the UTF-8-encoded source."""
    if span.end_byte > len(source_bytes):
        return ""
    return source_bytes[span.start_byte : span.end_byte].decode(
        "utf-8", errors="replace"
    )


def _function_complexities(
    source: str, language: str
) -> dict[str, tuple[int, list[str]]]:
    """Map function name -> (cyclomatic_complexity, source_lines)."""
    out: dict[str, tuple[int, list[str]]] = {}
    morph = ProgramMorphism(source=source, language=language)
    if not (morph.ast and morph.ast.uast_root):
        return out
    source_bytes = morph.source.encode("utf-8")
    try:
        callables = _collect_callables(morph.ast.uast_root)
    except Exception:
        return out
    for c in callables:
        name = c.attributes.get("name")
        if not name:
            for child in c.children:
                if child.kind == "Identifier":
                    name = _span_text(source_bytes, child.span)
                    break
        if not name:
            name = c.attributes.get("scope") or "anonymous"
        if name in out:
            continue
        try:
            blocks, edges, entry_id, exit_id = build_cfg_from_uast(c)
            cfg = ControlFlowGraph(
                blocks=blocks, edges=edges, entry_id=entry_id, exit_id=exit_id
            )
            complexity = cyclomatic_complexity(cfg)
        except Exception:
            continue
        body = _span_text(source_bytes, c.span)
        out[name] = (complexity, body.splitlines())
    return out


def _regression_diff(current_src: str, proposed_src: str, language: str) -> str | None:
    """Unified diff of the single function with the worst complexity increase."""
    cur = _function_complexities(current_src, language)
    prop = _function_complexities(proposed_src, language)
    if not cur or not prop:
        return None

    worst_name: str | None = None
    worst_delta = 0
    for name, (prop_cx, _) in prop.items():
        if name not in cur:
            continue
        delta = prop_cx - cur[name][0]
        if delta > worst_delta:
            worst_delta = delta
            worst_name = name
    if worst_name is None:
        return None

    cur_cx, cur_lines = cur[worst_name]
    prop_cx, prop_lines = prop[worst_name]
    diff_lines = list(
        difflib.unified_diff(
            cur_lines,
            prop_lines,
            fromfile=f"{worst_name} (current)",
            tofile=f"{worst_name} (proposed)",
            lineterm="",
        )
    )
    if not diff_lines:
        return None

    header = (
        f"# regression in `{worst_name}`: cyclomatic complexity "
        f"{cur_cx} -> {prop_cx} ({prop_cx - cur_cx:+d})"
    )
    body = diff_lines
    if len(body) > _REGRESSION_DIFF_MAX_LINES:
        hidden = len(body) - _REGRESSION_DIFF_MAX_LINES
        body = body[:_REGRESSION_DIFF_MAX_LINES]
        body.append(f"# ... (truncated, {hidden} more lines)")
    return "\\n".join([header, *body])


_STATUS_MEANING: dict[AssessmentStatus, str] = {
    AssessmentStatus.IMPROVEMENT: "moved up the lattice",
    AssessmentStatus.IMPROVEMENT_SCORE: "same verdict, scores improved",
    AssessmentStatus.LATERAL_MOVE: "no verdict or score movement",
    AssessmentStatus.REGRESSION: "moved down the lattice",
    AssessmentStatus.REGRESSION_SCORE: "same verdict, scores regressed",
    AssessmentStatus.SUSPICIOUS_NO_STRUCTURAL_CHANGE: (
        "scores moved but the AST barely changed"
    ),
}


def _render_deltas(r: AssessmentResult) -> list[str]:
    lines = []
    if r.score_deltas:
        deltas = ", ".join(f"{k}={v:+.1f}" for k, v in sorted(r.score_deltas.items()))
        lines.append(f"**Score deltas:** {deltas}")
    moved = {m: d for m, d in r.metric_deltas.items() if d != 0.0}
    if moved:
        md = ", ".join(f"`{m}`={d:+.3f}" for m, d in sorted(moved.items()))
        lines.append(f"**Metric deltas:** {md}")
    return lines


def render_assessment_md(r: AssessmentResult) -> str:
    """Compact markdown for a refactor assessment."""
    if r.error:
        return f"**Error:** {r.error}"
    meaning = _STATUS_MEANING.get(r.status, "")
    lines = [f"**Status:** {r.status.value} — {meaning}"]
    lines.append(f"**Priority:** `{r.priority.value}`")
    lines.append(
        f"**Verdict:** {r.current.lattice_element.value} → "
        f"{r.proposed.lattice_element.value}"
    )
    if r.structural_distance is not None:
        sim = f", similarity {r.similarity:.3f}" if r.similarity is not None else ""
        lines.append(f"**Structural distance:** {r.structural_distance:.3f}{sim}")
    if r.agent_contract is not None and (
        r.agent_contract.next_tool
        or r.agent_contract.next_actions
        or r.agent_contract.blocked_by
    ):
        lines.append("")
        lines.append("## Agent Contract")
        if r.agent_contract.next_tool:
            lines.append(f"- **Next tool:** `{r.agent_contract.next_tool}`")
        for action in r.agent_contract.next_actions:
            lines.append(f"- **Action:** {action}")
        for blocked in r.agent_contract.blocked_by:
            lines.append(f"- **Blocked by:** `{blocked}`")

    lines.extend(_render_deltas(r))

    if r.suspicion_reason:
        lines.append(f"> ⚠️ {r.suspicion_reason}")
    if r.regression_diff:
        lines.append("")
        lines.append("## Regression diff")
        lines.append("```diff")
        lines.append(r.regression_diff)
        lines.append("```")
    return "\\n".join(lines)
