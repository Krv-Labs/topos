"""
Response formatters for the Topos MCP server.

Converts ``ClassificationResult`` and distance results into the Pydantic
return models defined in ``schemas.py``. Also provides the Markdown flavor
used when ``response_format="markdown"``.
"""

from __future__ import annotations

from topos.logic.lattice import EvaluationValue
from topos.logic.omega import ClassificationResult
from topos.logic.policies.base import Priority
from .schemas import EvaluationResult, LatticeElement

_LATTICE_TO_STR: dict[EvaluationValue, LatticeElement] = {
    EvaluationValue.BROKEN: LatticeElement.BROKEN,
    EvaluationValue.COMPOSABLE: LatticeElement.COMPOSABLE,
    EvaluationValue.SELF_CONTAINED: LatticeElement.SELF_CONTAINED,
    EvaluationValue.SOUND: LatticeElement.SOUND,
}


def lattice_to_str(value: EvaluationValue) -> LatticeElement:
    return _LATTICE_TO_STR[value]


def build_guidance(result: ClassificationResult) -> str:
    """Priority-aware next-step hint for agents."""
    priority = result.priority
    s_score = result.scores.get("structural")
    c_score = result.scores.get("coupling")

    if priority == Priority.COMPOSABLE:
        if c_score is None:
            return (
                "Coupling not measured — provide a DependencyGraph "
                "(gitnexus_dir) for COMPOSABLE evaluation."
            )
        if c_score < 0.6:
            return (
                "Reduce coupling count and balance instability toward 0.3–0.7 "
                "to achieve COMPOSABLE."
            )
        return (
            "COMPOSABLE target achieved. Improve structural metrics "
            "(complexity, entropy) to reach SOUND."
        )

    if priority == Priority.SELF_CONTAINED:
        if s_score is not None and s_score < 0.6:
            return (
                "Reduce cyclomatic complexity and normalize entropy toward 0.5 "
                "to achieve SELF_CONTAINED."
            )
        return (
            "SELF_CONTAINED target achieved. Improve coupling "
            "(dependencies, instability) to reach SOUND."
        )

    # BALANCED
    hints: list[str] = []
    if s_score is not None and s_score < 0.6:
        hints.append("reduce complexity/entropy (structural)")
    if c_score is not None and c_score < 0.6:
        hints.append("reduce coupling (composability)")
    if c_score is None:
        hints.append("provide gitnexus_dir to enable coupling scoring")
    if hints:
        return "To improve: " + " and ".join(hints) + "."
    return "Code meets balanced quality targets."


def to_evaluation_result(
    result: ClassificationResult, coupling_available: bool
) -> EvaluationResult:
    """Convert a ``ClassificationResult`` into the Pydantic return model."""
    summary = result.summary()
    return EvaluationResult(
        is_parseable=result.is_parseable,
        lattice_element=lattice_to_str(summary),
        lattice_symbol=summary.symbol,
        lattice_description=summary.description,
        dimensions={dim: lattice_to_str(val) for dim, val in result.dimensions.items()},
        scores={dim: round(s * 100.0, 1) for dim, s in result.scores.items()},
        priority=result.priority,
        guidance=build_guidance(result),
        coupling_available=coupling_available,
        raw_metrics=dict(result.raw_metrics),
        interpretation=dict(result.interpretation),
    )


# ---------------------------------------------------------------------------
# Markdown renderers
# ---------------------------------------------------------------------------


def render_evaluation_md(e: EvaluationResult, title: str | None = None) -> str:
    lines: list[str] = []
    if title:
        lines.append(f"# {title}")
    lines.append(
        f"**Lattice:** {e.lattice_symbol} {e.lattice_element.value} — "
        f"{e.lattice_description}"
    )
    if not e.is_parseable:
        lines.append("> ⚠️ Code failed to parse.")
        return "\n".join(lines)

    lines.append("")
    lines.append("## Dimensions")
    for dim, val in e.dimensions.items():
        score = e.scores.get(dim, 0.0)
        lines.append(f"- **{dim}**: {val.value} ({score:.1f}%)")
    if not e.coupling_available:
        lines.append(
            "- _coupling: not measured (no DependencyGraph available — "
            "COMPOSABLE/SOUND unreachable)._"
        )

    lines.append("")
    lines.append(f"**Priority:** `{e.priority.value}`")
    lines.append(f"**Guidance:** {e.guidance}")

    if e.raw_metrics:
        lines.append("")
        lines.append("## Raw Metrics")
        for k, v in sorted(e.raw_metrics.items()):
            lines.append(f"- `{k}`: {v:.3f}")
    return "\n".join(lines)
