"""Shared markdown renderer for the ``topos_refactor_*`` tool family.

Mirrors how ``render_evaluation_md`` (``formatting.py``) is the single
renderer reused by every ``topos_evaluate_*`` tool: all three refactor tools
(cycles, dependencies, process) report the same "ranked hotspot" row shape,
so they share one table renderer instead of three near-duplicates.
"""

from __future__ import annotations

from .schemas import RefactorHotspot


def render_hotspots_md(title: str, hotspots: list[RefactorHotspot]) -> str:
    """Render a ranked list of :class:`RefactorHotspot` rows as markdown."""
    if not hotspots:
        return f"**{title}:** none found."

    lines = [f"## {title}", "", "| Kind | Label | Location | Score | Suggestion |"]
    lines.append("| --- | --- | --- | ---: | --- |")
    for h in hotspots:
        location = h.filepath
        if h.line_start is not None:
            location += f":{h.line_start}"
            if h.line_end is not None and h.line_end != h.line_start:
                location += f"-{h.line_end}"
        safe_label = h.label.replace("\n", " ").replace("|", "\\|")
        safe_suggestion = h.suggestion.replace("\n", " ").replace("|", "\\|")
        row = (
            f"| `{h.kind}` | `{safe_label}` | `{location}` | "
            f"{h.score:.3f} | {safe_suggestion} |"
        )
        lines.append(row)
    return "\n".join(lines)
