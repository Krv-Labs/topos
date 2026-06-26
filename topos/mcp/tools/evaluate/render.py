"""
Markdown helpers for evaluate tools.
"""

from __future__ import annotations

from ...schemas import EvaluationResult, ProjectEvaluationResult, ProjectFileEntry

def _render_project_entry(entry: ProjectFileEntry, verbose: bool) -> list[str]:
    lines = []
    s_str = ", ".join(f"{k}={v:.0f}" for k, v in entry.scores.items())
    lines.append(f"- `{entry.filepath}` — {entry.lattice_element.value} ({s_str})")
    if verbose and entry.raw_metrics:
        for k, v in sorted(entry.raw_metrics.items()):
            lines.append(f"  - `{k}`: {v:.3f}")
    return lines


def render_project_md(r: ProjectEvaluationResult) -> str:
    lines = [f"# Project Evaluation — {r.root}", ""]
    lines.append(f"**Overall:** {r.aggregate_floor_verdict.value}")
    lines.append(
        f"**Files scanned:** {r.file_count} (parse failures: {r.parse_failures})"
    )
    lines.append(f"**Priority:** `{r.priority.value}`")
    if not r.coupling_available:
        lines.append("> ⚠️ No `.gitnexus/` present — coupling dimension not scored.")
    if r.agent_contract is not None:
        lines.append("")
        lines.append("## Agent Contract")
        if r.agent_contract.next_tool:
            lines.append(f"- **Next tool:** `{r.agent_contract.next_tool}`")
        for action in r.agent_contract.next_actions:
            lines.append(f"- **Action:** {action}")
        for blocked in r.agent_contract.blocked_by:
            lines.append(f"- **Blocked by:** `{blocked}`")
    lines.append("")
    lines.append("## Rolled-up dimensions")
    for dim, val in r.rolled_up_dimensions.items():
        s = r.rolled_up_scores.get(dim)
        lines.append(
            f"- **{dim}**: {val.value}" + (f" ({s:.1f}%)" if s is not None else "")
        )
    lines.append("")
    lines.append(f"## Worst files (showing {r.count} of {r.total}, offset {r.offset})")
    for entry in r.files:
        lines.extend(_render_project_entry(entry, r.verbose))
    if r.has_more:
        lines.append(
            f"\\n_more files available: pass offset={r.next_offset} to continue._"
        )
    if r.error:
        lines.append(f"\\n> error: {r.error}")
    return "\\n".join(lines)

def _error_md(model: EvaluationResult) -> str:
    """Compact markdown for an error/early-return EvaluationResult."""
    return f"**Error:** {model.error or model.lattice_description}"
