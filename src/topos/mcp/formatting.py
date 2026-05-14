"""
Response formatters for the Topos MCP server.

Converts ``ClassificationResult`` and distance results into the Pydantic
return models defined in ``schemas.py``.  Also provides the Markdown
flavor used when ``response_format="markdown"``.
"""

from __future__ import annotations

from topos.logic.lattice import EvaluationValue
from topos.logic.omega import ClassificationResult
from topos.logic.policies.base import Priority

from .schemas import EvaluationResult, LatticeElement

_LATTICE_TO_STR: dict[EvaluationValue, LatticeElement] = {
    EvaluationValue.SLOP: LatticeElement.SLOP,
    EvaluationValue.SIMPLE: LatticeElement.SIMPLE,
    EvaluationValue.COMPOSABLE: LatticeElement.COMPOSABLE,
    EvaluationValue.SECURE: LatticeElement.SECURE,
    EvaluationValue.SIMPLE_COMPOSABLE: LatticeElement.SIMPLE_COMPOSABLE,
    EvaluationValue.SIMPLE_SECURE: LatticeElement.SIMPLE_SECURE,
    EvaluationValue.COMPOSABLE_SECURE: LatticeElement.COMPOSABLE_SECURE,
    EvaluationValue.IDEAL: LatticeElement.IDEAL,
}


def lattice_to_str(value: EvaluationValue) -> LatticeElement:
    return _LATTICE_TO_STR[value]


def build_guidance(result: ClassificationResult) -> str:
    """Priority-aware next-step hint for agents.

    Phrased against the three free generators (SIMPLE, COMPOSABLE, SECURE).
    """
    priority = result.priority
    simple_score = result.scores.get("simple")
    composable_score = result.scores.get("composable")
    secure_score = result.scores.get("secure")

    if priority == Priority.COMPOSABLE:
        if composable_score is None:
            return (
                "COMPOSABLE not measured — provide a DependencyGraph "
                "(gitnexus_dir) to score the composable generator."
            )
        if composable_score < 0.6:
            return (
                "Reduce coupling count and balance instability toward 0.3–0.7 "
                "to satisfy COMPOSABLE."
            )
        return (
            "COMPOSABLE satisfied.  Reduce CFG cyclomatic complexity and "
            "address any CPG security findings to reach IDEAL."
        )

    if priority == Priority.SIMPLE:
        if simple_score is not None and simple_score < 0.6:
            return (
                "Reduce CFG cyclomatic complexity (split branches; lift "
                "guard clauses) to satisfy SIMPLE."
            )
        return (
            "SIMPLE satisfied.  Add COMPOSABLE / SECURE checks to reach IDEAL."
        )

    if priority == Priority.SECURE:
        if secure_score is not None and secure_score < 0.6:
            return (
                "Eliminate dangerous-API calls and source→sink taint flows "
                "to satisfy SECURE."
            )
        return (
            "SECURE satisfied.  Address SIMPLE / COMPOSABLE generators "
            "to reach IDEAL."
        )

    # BALANCED
    hints: list[str] = []
    if simple_score is not None and simple_score < 0.6:
        hints.append("reduce CFG cyclomatic complexity (SIMPLE)")
    if composable_score is not None and composable_score < 0.6:
        hints.append("reduce coupling (COMPOSABLE)")
    if composable_score is None:
        hints.append("provide gitnexus_dir to enable COMPOSABLE scoring")
    if secure_score is not None and secure_score < 0.6:
        hints.append("eliminate dangerous-API calls / taint paths (SECURE)")
    if hints:
        return "To improve: " + " and ".join(hints) + "."
    return "All three generators satisfied — code is at IDEAL."


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
    lines.append("## Generators")
    for dim, val in e.dimensions.items():
        score = e.scores.get(dim, 0.0)
        lines.append(f"- **{dim}**: {val.value} ({score:.1f}%)")
    if not e.coupling_available:
        lines.append(
            "- _composable: not measured (no DependencyGraph available — "
            "COMPOSABLE / IDEAL unreachable)._"
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
