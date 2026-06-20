"""
Response formatters for the Topos MCP server.

Converts ``ClassificationResult`` and distance results into the Pydantic
return models defined in ``schemas.py``.
"""

from __future__ import annotations

from fastmcp.tools.base import ToolResult
from pydantic import BaseModel

from topos.core.omega import EvaluationValue
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.policies.base import Priority
from topos.evaluation.preferences import UserPreferences
from topos.evaluation.suggestions import suggest_refactors

from .schemas import (
    EvaluationResult,
    LatticeElement,
    PillarResult,
    PreferenceWalk,
    PrioritySource,
    SecurityFinding,
    Suggestion,
)

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


# Raw metrics are namespaced by representation: cfg/ast -> simple,
# mdg -> composable, cpg -> secure.  (pdg.* is structural and maps to no
# pillar; it is preserved only in the flat ``raw_metrics``.)
_PILLAR_METRIC_PREFIXES: dict[str, tuple[str, ...]] = {
    "simple": ("cfg.", "ast."),
    "composable": ("mdg.",),
    "secure": ("cpg.",),
}


def mdg_unavailable_message(warnings: list[str] | None) -> str:
    """The 'COMPOSABLE not scored' note surfaced when no MDG is available."""
    return (warnings or [None])[0] or (
        "unavailable — no ModuleDependencyGraph; run 'topos depgraph generate' "
        "to score COMPOSABLE."
    )


def build_pillars(
    result: ClassificationResult,
    coupling_available: bool,
    *,
    warnings: list[str] | None = None,
) -> dict[str, PillarResult]:
    """Build the lean per-pillar (simple, composable, secure) summary.

    Each pillar carries only ``achieved`` + ``score``.  The full metric and
    interpretation detail is NOT duplicated here — it lives once in the
    parent's flat ``raw_metrics`` / ``interpretation`` (see
    ``_PILLAR_METRIC_PREFIXES`` for the namespacing).  ``warnings`` is unused
    now that the ``mdg.unavailable`` note is injected into the flat
    interpretation by ``to_evaluation_result``; kept for call-site stability.
    """
    pillars: dict[str, PillarResult] = {}
    for dim, metric_prefixes in _PILLAR_METRIC_PREFIXES.items():
        has_metrics = any(
            any(k.startswith(p) for p in metric_prefixes) for k in result.raw_metrics
        )

        # achieved = was the singleton generator for this dimension satisfied?
        generator_val = {
            "simple": EvaluationValue.SIMPLE,
            "composable": EvaluationValue.COMPOSABLE,
            "secure": EvaluationValue.SECURE,
        }[dim]

        achieved = result.dimensions.get(dim) == generator_val
        score = result.scores.get(dim, 0.0)

        # Only include pillars that were actually measured (composable still
        # surfaces when coupling is unavailable, to carry achieved=False).
        if has_metrics or dim == "composable" and not coupling_available:
            pillars[dim] = PillarResult(
                achieved=achieved,
                score=round(score * 100.0, 1),
            )
    return pillars


def _failing_interpretation(
    result: ClassificationResult, interpretation: dict[str, str]
) -> dict[str, str]:
    """Keep only interpretation strings for generators that were NOT satisfied.

    Default (non-verbose) output drops the 16 raw-metric floats but must still
    tell the agent *why* a failing generator failed. We retain interpretation
    keys whose representation prefix maps (via ``_PILLAR_METRIC_PREFIXES``) to a
    dimension that did not achieve its singleton generator, plus any non-pillar
    note (e.g. ``mdg.unavailable``) that carries no prefix mapping.
    """
    achieved: dict[str, bool] = {}
    for dim in _PILLAR_METRIC_PREFIXES:
        generator_val = {
            "simple": EvaluationValue.SIMPLE,
            "composable": EvaluationValue.COMPOSABLE,
            "secure": EvaluationValue.SECURE,
        }[dim]
        achieved[dim] = result.dimensions.get(dim) == generator_val

    def _dim_for(key: str) -> str | None:
        for dim, prefixes in _PILLAR_METRIC_PREFIXES.items():
            if any(key.startswith(p) for p in prefixes):
                return dim
        return None

    kept: dict[str, str] = {}
    for key, text in interpretation.items():
        dim = _dim_for(key)
        # Keep notes with no pillar mapping (e.g. mdg.unavailable) and any
        # interpretation belonging to a generator that failed.
        if dim is None or not achieved.get(dim, False):
            kept[key] = text
    return kept


def to_evaluation_result(
    result: ClassificationResult,
    coupling_available: bool,
    *,
    preferences: UserPreferences | None = None,
    priority_source: PrioritySource = PrioritySource.DEFAULT,
    warnings: list[str] | None = None,
    security_findings: list[SecurityFinding] | None = None,
    verbose: bool = True,
) -> EvaluationResult:
    """Convert a ``ClassificationResult`` into the Pydantic return model.

    When ``verbose`` is ``False`` the structured channel omits the 16 raw-metric
    floats and trims ``interpretation`` to failing generators only (see
    ``_failing_interpretation``). Measurement showed ``raw_metrics`` +
    ``interpretation`` make up ~55% of the default structured payload, and
    clients routinely inject ``structured_content`` into context, so gating both
    channels (not just the markdown) is what earns the keep here.
    """
    summary = result.summary()
    walk = (
        build_preference_walk(preferences, summary) if preferences is not None else None
    )

    interpretation = dict(result.interpretation)
    if not coupling_available:
        # The "COMPOSABLE not scored" note has no flat-metric equivalent; it
        # used to live only on the composable pillar's interpretation. Keep it
        # reaching the agent by parking it in the single flat interpretation.
        interpretation.setdefault("mdg.unavailable", mdg_unavailable_message(warnings))

    raw_metrics = dict(result.raw_metrics)
    if not verbose:
        interpretation = _failing_interpretation(result, interpretation)
        raw_metrics = {}

    # TODO(allowlist): the MCP layer does not yet load a ``ToposConfig``, so the
    # ``security_findings`` passed in are RAW (no allowlist applied). Passing
    # them as ``active_findings`` is the correct no-config path. To surface
    # allowlist-adjusted verdicts: load ToposConfig, call
    # ``apply_allowlist(result, findings, config)`` (suppression.py), pass the
    # returned ``AdjustedVerdict.active_findings`` here, and surface the
    # raw-vs-adjusted distinction on the result.
    suggestions = [
        Suggestion(
            pillar=s.pillar,
            metric=s.metric,
            severity=s.severity,
            message=s.message,
        )
        for s in suggest_refactors(result, active_findings=security_findings)
    ]

    return EvaluationResult(
        is_parseable=result.is_parseable,
        lattice_element=lattice_to_str(summary),
        lattice_symbol=summary.symbol,
        lattice_description=summary.description,
        dimensions={dim: lattice_to_str(val) for dim, val in result.dimensions.items()},
        scores={dim: round(s * 100.0, 1) for dim, s in result.scores.items()},
        pillars=build_pillars(result, coupling_available, warnings=warnings),
        priority=result.priority,
        priority_source=priority_source,
        guidance=build_guidance(result),
        coupling_available=coupling_available,
        raw_metrics=raw_metrics,
        interpretation=interpretation,
        warnings=warnings or [],
        security_findings=security_findings or [],
        suggestions=suggestions,
        preference_walk=walk,
    )


# ---------------------------------------------------------------------------
# Dual-channel converter (Software 3.0)
# ---------------------------------------------------------------------------


def to_tool_result(model: BaseModel, markdown: str) -> ToolResult:
    """Return a dual-channel ``ToolResult``: markdown for the LLM, JSON for code.

    ``content`` is the compact markdown the LLM reads (FastMCP wraps the bare
    string in a ``TextContent``); ``structured_content`` is the model's
    JSON-mode dump for programmatic clients.

    Empirically verified FastMCP behavior (see ``tests/mcp/test_evaluate.py``
    and a probe over ``mcp.call_tool`` + ``to_mcp_tool().model_dump()``):
      * When a tool is annotated ``-> ToolResult``, FastMCP emits **no**
        ``outputSchema`` for it — this is intentional and shrinks the eager
        tool-definition surface (outputSchema is optional per the MCP spec).
        Tools wanting to keep an outputSchema can instead annotate ``-> Model``
        while still returning this ``ToolResult`` (both channels are honored at
        runtime regardless of the annotation).
      * ``structured_content`` is still delivered on the wire.
      * ``content`` is the markdown text, not the model JSON.
    """
    return ToolResult(
        content=markdown, structured_content=model.model_dump(mode="json")
    )


# ---------------------------------------------------------------------------
# Markdown renderers
# ---------------------------------------------------------------------------


def render_evaluation_md(
    e: EvaluationResult, title: str | None = None, *, verbose: bool = True
) -> str:
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

    # Suggestions are the actionable payload — show them by default (one line
    # each, already concise). Not verbose-gated: they are the whole point.
    if e.suggestions:
        lines.append("")
        lines.append("## Suggestions")
        for s in e.suggestions:
            lines.append(f"- [ ] ({s.pillar}) {s.message}")

    # The 16 raw-metric floats are the heaviest part of the default markdown
    # (~430 of ~794 bytes for a typical file) and are rarely needed inline;
    # gate them behind ``verbose``.
    if verbose and e.raw_metrics:
        lines.append("")
        lines.append("## Raw Metrics")
        for k, v in sorted(e.raw_metrics.items()):
            lines.append(f"- `{k}`: {v:.3f}")
    return "\n".join(lines)
