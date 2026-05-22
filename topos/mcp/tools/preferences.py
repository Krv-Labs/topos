"""
``topos_preference_walk`` — convert a strict generator ordering into a
concrete relaxation walk on Ω.

This tool is the agent's explicit handle on the preference machinery:
given a ranking (and optionally a current verdict), it returns the
descending preference-ordered sequence of lattice verdicts to aim for,
annotated with the satisfied-generator sets so the agent can see at a
glance what changing to each step requires.

Purely a computation — no source code, no I/O.  Cheap to call before
every refactor iteration to refresh the agent's next goal.
"""

from __future__ import annotations

from topos.core.omega import EvaluationValue
from topos.evaluation.preferences import Generator, UserPreferences

from ..formatting import lattice_to_str, str_to_lattice
from ..schemas import (
    LatticeElement,
    PreferenceWalkInput,
    PreferenceWalkResult,
    ResponseFormat,
    WalkStep,
)
from ..server import mcp

_READ_ONLY_ANN = {
    "title": "Topos Preference Walk",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}


def _generators_satisfied(value: EvaluationValue) -> list[Generator]:
    """Decode an Ω element into its satisfied-generator subset."""
    bits = int(value)
    out: list[Generator] = []
    if bits & 0b001:
        out.append(Generator.SIMPLE)
    if bits & 0b010:
        out.append(Generator.COMPOSABLE)
    if bits & 0b100:
        out.append(Generator.SECURE)
    return out


def _to_step(prefs: UserPreferences, value: EvaluationValue) -> WalkStep:
    return WalkStep(
        verdict=lattice_to_str(value),
        preference_score=prefs.score(value),
        generators_satisfied=_generators_satisfied(value),
    )


@mcp.tool(
    name="topos_preference_walk",
    tags={"preferences", "workflow"},
    annotations=_READ_ONLY_ANN,
)
def topos_preference_walk(params: PreferenceWalkInput) -> PreferenceWalkResult:
    """Convert a generator ranking into a concrete relaxation walk on Ω.

    The walk is the descending preference-ordered list of Ω verdicts
    starting at the aspirational target (default: ``IDEAL``) down to
    (but not including) the current verdict.  By convention the
    **second** element of the walk is the ``fallback_target`` — the
    meet of the top-two ranked generators (the "ideal intersection"),
    which is the natural divert-point when IDEAL plateaus.

    Each step is annotated with the satisfied-generator set, so the
    agent can see what changing to that verdict requires (e.g. "next
    step adds COMPOSABLE").

    No source code is required — this is purely a computation over
    the preference ordering.  Call it between refactor iterations to
    refresh the agent's concrete next goal.
    """
    try:
        target_value = (
            str_to_lattice(params.target) if params.target is not None else None
        )
        prefs = UserPreferences.from_iterable(params.ranking, target=target_value)
    except (ValueError, KeyError) as exc:
        return PreferenceWalkResult(
            ranking=list(params.ranking),
            aspirational_target=LatticeElement.IDEAL,
            fallback_target=LatticeElement.IDEAL,
            current=params.current,
            next_step=None,
            progress=0.0,
            walk=[],
            induced_order=[],
            warnings=_response_format_warnings(params.response_format),
            error=str(exc),
        )

    current_value = (
        str_to_lattice(params.current) if params.current is not None else None
    )
    walk_values = prefs.relaxation_walk(current_value)
    next_value = prefs.next_step(current_value) if current_value is not None else None
    progress = (
        round(prefs.progress(current_value), 3) if current_value is not None else 0.0
    )

    return PreferenceWalkResult(
        ranking=list(prefs.ranking),
        aspirational_target=lattice_to_str(prefs.aspirational_target()),
        fallback_target=lattice_to_str(prefs.fallback_target()),
        current=params.current,
        next_step=lattice_to_str(next_value) if next_value is not None else None,
        progress=progress,
        walk=[_to_step(prefs, v) for v in walk_values],
        induced_order=[_to_step(prefs, v) for v in prefs.induced_total_order()],
        warnings=_response_format_warnings(params.response_format),
    )


def render_preference_walk_md(r: PreferenceWalkResult) -> str:
    """Markdown rendering of a preference-walk result for agent UIs."""
    ranking_str = " ≻ ".join(g.value for g in r.ranking)
    lines = [
        "# Preference Walk",
        f"**Ranking:** {ranking_str}",
        f"**Aspirational target:** `{r.aspirational_target.value}` (aim here first)",
        f"**Fallback target:** `{r.fallback_target.value}` — divert here if "
        "the aspirational target plateaus",
    ]
    if r.current is not None:
        lines.append(f"**Current verdict:** `{r.current.value}`")
        lines.append(f"**Progress to target:** {r.progress * 100:.0f}%")
        if r.next_step is not None:
            lines.append(f"**Immediate next step:** `{r.next_step.value}`")
        else:
            lines.append("_Already at or beyond the aspirational target — no walk._")
    if r.walk:
        lines.append("\n## Walk (descending preference)")
        for s in r.walk:
            sat = ", ".join(g.value for g in s.generators_satisfied) or "—"
            lines.append(
                f"- `{s.verdict.value}` (score {s.preference_score}) — satisfies: {sat}"
            )
    lines.append("\n## Full induced order on Ω")
    for s in r.induced_order:
        sat = ", ".join(g.value for g in s.generators_satisfied) or "—"
        lines.append(
            f"- `{s.verdict.value}` (score {s.preference_score}) — satisfies: {sat}"
        )
    if r.error:
        lines.append(f"\n> error: {r.error}")
    return "\n".join(lines)


def _response_format_warnings(response_format: ResponseFormat) -> list[str]:
    if response_format == ResponseFormat.MARKDOWN:
        return []
    return [
        "response_format is deprecated/no-op for MCP structured output; tools return "
        "structured content regardless of this value."
    ]
