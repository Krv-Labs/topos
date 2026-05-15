"""
Response formatters for the Topos MCP server.

Converts ``ClassificationResult`` and distance results into the Pydantic
return models defined in ``schemas.py``.  Also provides the Markdown
flavor used when ``response_format="markdown"``.
"""

from __future__ import annotations

from topos.core.omega import EvaluationValue
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.policies.base import Priority
from topos.evaluation.preferences import UserPreferences

from .schemas import EvaluationResult, LatticeElement, PillarResult, PreferenceWalk

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


_STR_TO_LATTICE: dict[LatticeElement, EvaluationValue] = {
    v: k for k, v in _LATTICE_TO_STR.items()
}


def lattice_to_str(value: EvaluationValue) -> LatticeElement:
    return _LATTICE_TO_STR[value]


def str_to_lattice(value: LatticeElement) -> EvaluationValue:
    return _STR_TO_LATTICE[value]


def build_preference_walk(
    prefs: UserPreferences, current: EvaluationValue
) -> PreferenceWalk:
    """Materialize a ``PreferenceWalk`` for the result schema."""
    target = prefs.aspirational_target()
    fallback = prefs.fallback_target()
    walk = prefs.relaxation_walk(current)
    nxt = prefs.next_step(current)
    return PreferenceWalk(
        ranking=list(prefs.ranking),
        target=lattice_to_str(target),
        fallback_target=lattice_to_str(fallback),
        walk=[lattice_to_str(v) for v in walk],
        next_step=lattice_to_str(nxt) if nxt is not None else None,
        progress=round(prefs.progress(current), 3),
    )


def build_guidance(result: ClassificationResult) -> str:
    """Priority-aware next-step hint for agents.

    Phrased against the three free generators (SIMPLE, COMPOSABLE, SECURE).
    """
    priority = result.priority
    simple_ok = result.dimensions.get("simple") == EvaluationValue.SIMPLE
    composable_ok = result.dimensions.get("composable") == EvaluationValue.COMPOSABLE
    secure_ok = result.dimensions.get("secure") == EvaluationValue.SECURE

    if priority == Priority.COMPOSABLE:
        if "composable" not in result.dimensions:
            return (
                "COMPOSABLE not measured — provide a ModuleDependencyGraph "
                "(gitnexus_dir) to score the composable generator."
            )
        if not composable_ok:
            return (
                "Balance instability (aim for 0.3–0.7) and reduce fan-in/fan-out "
                "(aim for <= 15) to satisfy COMPOSABLE."
            )
        return (
            "COMPOSABLE satisfied.  Simplify CFG/functions and "
            "address any CPG security findings to reach GOLD."
        )

    if priority == Priority.SIMPLE:
        if not simple_ok:
            return (
                "Reduce CFG/function cyclomatic complexity (aim for <= 15/10) "
                "and ensure AST entropy is structured (0.2–0.8) to satisfy SIMPLE."
            )
        return "SIMPLE satisfied.  Add COMPOSABLE / SECURE checks to reach GOLD."

    # priority == Priority.SECURE  (only remaining case after exhaustive match)
    if not secure_ok:
        return (
            "Eliminate all dangerous-API calls and source→sink taint flows "
            "to satisfy SECURE."
        )
    return "SECURE satisfied.  Address SIMPLE / COMPOSABLE generators to reach GOLD."


def build_pillars(
    result: ClassificationResult, coupling_available: bool
) -> dict[str, PillarResult]:
    """Build the per-pillar (simple, composable, secure) breakdown."""
    pillars: dict[str, PillarResult] = {}
    for dim in ("simple", "composable", "secure"):
        # raw metrics namespaced by representation: cfg/ast -> simple,
        # mdg -> composable, cpg -> secure
        metric_prefixes = {
            "simple": ("cfg.", "ast."),
            "composable": ("mdg.",),
            "secure": ("cpg.",),
        }[dim]

        dim_metrics = {
            k: v
            for k, v in result.raw_metrics.items()
            if any(k.startswith(p) for p in metric_prefixes)
        }
        dim_interp = {
            k: v
            for k, v in result.interpretation.items()
            if any(k.startswith(p) for p in metric_prefixes)
        }

        # achieved = was the singleton generator for this dimension satisfied?
        generator_val = {
            "simple": EvaluationValue.SIMPLE,
            "composable": EvaluationValue.COMPOSABLE,
            "secure": EvaluationValue.SECURE,
        }[dim]

        achieved = result.dimensions.get(dim) == generator_val
        score = result.scores.get(dim, 0.0)

        # Only include pillars that were actually measured
        if dim_metrics or dim == "composable" and not coupling_available:
            pillars[dim] = PillarResult(
                achieved=achieved,
                score=round(score * 100.0, 1),
                metrics=dim_metrics,
                interpretation=dim_interp,
            )
    return pillars


def to_evaluation_result(
    result: ClassificationResult,
    coupling_available: bool,
    *,
    preferences: UserPreferences | None = None,
) -> EvaluationResult:
    """Convert a ``ClassificationResult`` into the Pydantic return model."""
    summary = result.summary()
    walk = (
        build_preference_walk(preferences, summary) if preferences is not None else None
    )

    return EvaluationResult(
        is_parseable=result.is_parseable,
        lattice_element=lattice_to_str(summary),
        lattice_symbol=summary.symbol,
        lattice_description=summary.description,
        dimensions={dim: lattice_to_str(val) for dim, val in result.dimensions.items()},
        scores={dim: round(s * 100.0, 1) for dim, s in result.scores.items()},
        pillars=build_pillars(result, coupling_available),
        priority=result.priority,
        guidance=build_guidance(result),
        coupling_available=coupling_available,
        raw_metrics=dict(result.raw_metrics),
        interpretation=dict(result.interpretation),
        preference_walk=walk,
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
            "- _composable: not measured (no ModuleDependencyGraph available — "
            "COMPOSABLE / IDEAL unreachable)._"
        )

    lines.append("")
    lines.append(f"**Priority:** `{e.priority.value}`")
    lines.append(f"**Guidance:** {e.guidance}")

    if e.preference_walk is not None:
        pw = e.preference_walk
        ranking = " ≻ ".join(g.value for g in pw.ranking)
        lines.append("")
        lines.append("## Preference Walk")
        lines.append(f"- **Ranking:** {ranking}")
        lines.append(
            f"- **Target (aspirational):** {pw.target.value} "
            f"({pw.progress * 100:.0f}% of the way)"
        )
        lines.append(
            f"- **Fallback (ideal intersection):** {pw.fallback_target.value} "
            "— divert here if IDEAL plateaus"
        )
        if pw.next_step is not None:
            lines.append(f"- **Next step:** aim for `{pw.next_step.value}`")
        if pw.walk:
            walk_str = " → ".join(v.value for v in pw.walk)
            lines.append(f"- **Walk:** {walk_str}")
        else:
            lines.append("- **Walk:** _at or beyond target — no further steps._")

    if e.raw_metrics:
        lines.append("")
        lines.append("## Raw Metrics")
        for k, v in sorted(e.raw_metrics.items()):
            lines.append(f"- `{k}`: {v:.3f}")
    return "\n".join(lines)
