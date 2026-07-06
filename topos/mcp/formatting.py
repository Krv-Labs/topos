"""
Response formatters for the Topos MCP server.

Converts ``ClassificationResult`` and distance results into the Pydantic
return models defined in ``schemas.py``.
"""

from __future__ import annotations

from dataclasses import dataclass, replace

from fastmcp.tools.base import ToolResult
from pydantic import BaseModel

from topos.core.omega import EvaluationValue
from topos.evaluation.characteristic_morphism import ClassificationResult
from topos.evaluation.policies.base import Priority
from topos.evaluation.policies.gates import (
    PILLAR_METRIC_PREFIXES as _PILLAR_METRIC_PREFIXES,
)
from topos.evaluation.preferences import UserPreferences
from topos.evaluation.suggestions import suggest_refactors

from .evaluation import INVALID_GITNEXUS_MARKERS, STALE_GITNEXUS_MARKER
from .schemas import (
    AcknowledgedRisk,
    AgentContract,
    EvaluationResult,
    FunctionEntry,
    LatticeElement,
    PillarResult,
    PreferenceWalk,
    PrioritySource,
    RefactorTarget,
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


@dataclass(frozen=True)
class ComposableContractSignals:
    """Agent-contract fields implied by COMPOSABLE setup state."""

    blocked_by: list[str]
    risk_flags: list[str]
    next_tool: str | None = None
    next_action: str | None = None


def lattice_to_str(value: EvaluationValue) -> LatticeElement:
    return _LATTICE_TO_STR[value]


def str_to_lattice(value: LatticeElement) -> EvaluationValue:
    return _STR_TO_LATTICE[value]


def composable_contract_signals(
    *,
    coupling_available: bool,
    warnings: list[str] | None = None,
    include_missing: bool = True,
) -> ComposableContractSignals:
    """Classify COMPOSABLE setup blockers from the shared warning markers."""
    blocked_by: list[str] = []
    risk_flags: list[str] = []
    messages = warnings or []

    invalid_override = any(
        marker in w for w in messages for marker in INVALID_GITNEXUS_MARKERS
    )
    stale_graph = any(STALE_GITNEXUS_MARKER in w for w in messages)

    if not coupling_available:
        if invalid_override:
            blocked_by.append("invalid_gitnexus_dir")
            risk_flags.append("invalid_gitnexus_dir")
        elif include_missing:
            blocked_by.append("missing_gitnexus_dir")
        if invalid_override or include_missing:
            risk_flags.append("composable_unavailable")

    if stale_graph:
        blocked_by.append("stale_gitnexus_dir")
        risk_flags.append("stale_gitnexus_dir")

    if "invalid_gitnexus_dir" in blocked_by:
        return ComposableContractSignals(
            blocked_by=blocked_by,
            risk_flags=risk_flags,
            next_action=(
                "fix gitnexus_dir — it must be an existing directory inside "
                "the file root"
            ),
        )
    if "stale_gitnexus_dir" in blocked_by:
        return ComposableContractSignals(
            blocked_by=blocked_by,
            risk_flags=risk_flags,
            next_tool="topos_generate_depgraph",
            next_action="run topos_generate_depgraph to refresh COMPOSABLE",
        )
    return ComposableContractSignals(blocked_by=blocked_by, risk_flags=risk_flags)


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


def build_agent_contract(
    result: ClassificationResult,
    *,
    coupling_available: bool,
    security_findings: list[SecurityFinding],
    acknowledged_risks: list[AcknowledgedRisk],
    grade_capped: bool,
    warnings: list[str] | None = None,
    refactor_targets: list[RefactorTarget] | None = None,
    offer_refactor_targets: bool = False,
) -> tuple[str | None, list[str], list[str], list[str], list[str]]:
    """Return compact loop-control fields for MCP agents.

    ``refactor_targets`` are ranked edit targets the caller computed (None
    when the tool does not support them or they were not requested);
    ``offer_refactor_targets`` marks a tool that supports them but was called
    without them, so a below-IDEAL result can advertise the option.

    Invariant: ``next_tool``/``next_actions`` never contradict ``blocked_by``
    — when a target coexists with a setup blocker, ``next_actions`` carries
    both the edit step and the setup remedy.
    """
    blocked_by: list[str] = []
    risk_flags: list[str] = []
    next_actions: list[str] = []

    if not result.is_parseable:
        blocked_by.append("parse_failure")
        risk_flags.append("parse_failure")
        return None, [], blocked_by, ["restore parseable source"], risk_flags

    composable = composable_contract_signals(
        coupling_available=coupling_available,
        warnings=warnings,
    )
    blocked_by.extend(composable.blocked_by)
    risk_flags.extend(composable.risk_flags)
    if security_findings:
        risk_flags.append("active_security_findings")
    if acknowledged_risks:
        risk_flags.append("acknowledged_security_risk")
    if grade_capped:
        risk_flags.append("grade_capped")
    if warnings:
        risk_flags.append("warnings")

    summary = result.summary()
    simple_ok = result.dimensions.get("simple") == EvaluationValue.SIMPLE

    if refactor_targets:
        # Ranked edit targets were requested and found: the edit loop is the
        # next step. Setup blockers stay in blocked_by and their remedies ride
        # along in next_actions so the contract never contradicts itself.
        first = refactor_targets[0]
        next_tool = "topos_assess_worktree_change"
        next_actions.append(
            f"edit target {first.target_id} ({first.metric}) — "
            "one focused structural change"
        )
        if composable.next_action:
            next_actions.append(composable.next_action)
        elif "missing_gitnexus_dir" in blocked_by:
            next_actions.append("run topos_generate_depgraph to score COMPOSABLE")
    elif composable.next_action:
        next_tool = composable.next_tool
        next_actions.append(composable.next_action)
    elif summary == EvaluationValue.IDEAL:
        next_tool = "topos_evaluate_project"
        next_actions.append(
            "confirm project rollup and behavior tests before accepting"
        )
    elif not simple_ok:
        next_tool = "topos_inspect_code"
        next_actions.append(
            "inspect weakest measured pillar, then verify a focused patch"
        )
    elif security_findings:
        next_tool = "topos_inspect_code"
        next_actions.append(
            "remove active SECURE findings or acknowledge intentional risk"
        )
    elif "missing_gitnexus_dir" in blocked_by:
        next_tool = "topos_generate_depgraph"
        next_actions.append("run topos_generate_depgraph to score COMPOSABLE")
    else:
        next_tool = "topos_inspect_code"
        next_actions.append(
            "inspect weakest measured pillar, then verify a focused patch"
        )

    if offer_refactor_targets and summary != EvaluationValue.IDEAL:
        # Self-discoverability: the caller supports ranked targets but did not
        # ask for them; a below-IDEAL verdict is the moment they help.
        next_actions.append(
            "re-run topos_evaluate_file with refactor_targets=5 for ranked edit targets"
        )

    verification_gates = [
        "verify in-place edits with topos_assess_worktree_change",
        "assessment status is IMPROVEMENT or IMPROVEMENT_SCORE",
        "assessment status is not SUSPICIOUS_NO_STRUCTURAL_CHANGE",
        "behavior tests or type/lint checks pass when available",
    ]
    return next_tool, next_actions, blocked_by, verification_gates, risk_flags


# Raw metrics are namespaced by representation: cfg/ast -> simple,
# mdg -> composable, cpg -> secure.  (pdg.* is structural and maps to no
# pillar; it is preserved only in the flat ``raw_metrics``.)
# _PILLAR_METRIC_PREFIXES is imported from topos.evaluation.policies.gates —
# the canonical metric-namespace map shared with gate specs and refactor
# targets.


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
    acknowledged_risks: list[AcknowledgedRisk] | None = None,
    adjusted_verdict=None,
    include_agent_contract: bool = True,
    verbose: bool = True,
    metric_locations: dict[str, list[FunctionEntry]] | None = None,
    refactor_targets: list[RefactorTarget] | None = None,
    offer_refactor_targets: bool = False,
) -> EvaluationResult:
    """Convert a ``ClassificationResult`` into the Pydantic return model.

    When ``verbose`` is ``False`` the structured channel omits the 16 raw-metric
    floats and trims ``interpretation`` to failing generators only (see
    ``_failing_interpretation``). Measurement showed ``raw_metrics`` +
    ``interpretation`` make up ~55% of the default structured payload, and
    clients routinely inject ``structured_content`` into context, so gating both
    channels (not just the markdown) is what earns the keep here.
    """
    summary = (
        adjusted_verdict.adjusted_element if adjusted_verdict else result.summary()
    )
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

    dimensions = dict(result.dimensions)
    if adjusted_verdict is not None and adjusted_verdict.adjusted_secure_pass:
        dimensions["secure"] = EvaluationValue.SECURE
    display_result = replace(result, dimensions=dimensions, lattice_element=summary)

    active_findings = security_findings or []
    risks = acknowledged_risks or []
    grade_capped = adjusted_verdict.grade_capped if adjusted_verdict else False
    agent_contract = None
    if include_agent_contract:
        next_tool, next_actions, blocked_by, verification_gates, risk_flags = (
            build_agent_contract(
                display_result,
                coupling_available=coupling_available,
                security_findings=active_findings,
                acknowledged_risks=risks,
                grade_capped=grade_capped,
                warnings=warnings,
                refactor_targets=refactor_targets,
                offer_refactor_targets=offer_refactor_targets,
            )
        )
        agent_contract = AgentContract(
            next_tool=next_tool,
            next_actions=next_actions,
            blocked_by=blocked_by,
            verification_gates=verification_gates,
            risk_flags=risk_flags,
        )

    suggestions = [
        Suggestion(
            pillar=s.pillar,
            metric=s.metric,
            severity=s.severity,
            message=s.message,
        )
        for s in suggest_refactors(result, active_findings=active_findings)
    ]

    return EvaluationResult(
        is_parseable=result.is_parseable,
        lattice_element=lattice_to_str(summary),
        lattice_symbol=summary.symbol,
        lattice_description=summary.description,
        dimensions={dim: lattice_to_str(val) for dim, val in dimensions.items()},
        scores={dim: round(s * 100.0, 1) for dim, s in result.scores.items()},
        pillars=build_pillars(display_result, coupling_available, warnings=warnings),
        priority=result.priority,
        priority_source=priority_source,
        guidance=build_guidance(display_result),
        coupling_available=coupling_available,
        raw_metrics=raw_metrics,
        interpretation=interpretation,
        metric_locations=metric_locations or {},
        warnings=warnings or [],
        agent_contract=agent_contract,
        security_findings=active_findings,
        acknowledged_risks=risks,
        raw_lattice_element=(
            lattice_to_str(adjusted_verdict.raw_element) if adjusted_verdict else None
        ),
        adjusted_lattice_element=(
            lattice_to_str(adjusted_verdict.adjusted_element)
            if adjusted_verdict
            else None
        ),
        secure_raw=adjusted_verdict.raw_secure_pass if adjusted_verdict else None,
        secure_adjusted=(
            adjusted_verdict.adjusted_secure_pass if adjusted_verdict else None
        ),
        grade_capped=grade_capped,
        suggestions=suggestions,
        refactor_targets=refactor_targets or [],
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
    if e.agent_contract is not None:
        contract = e.agent_contract
        if contract.next_tool or contract.next_actions or contract.blocked_by:
            lines.append("")
            lines.append("## Agent Contract")
            if contract.next_tool:
                lines.append(f"- **Next tool:** `{contract.next_tool}`")
            for action in contract.next_actions:
                lines.append(f"- **Action:** {action}")
            for blocked in contract.blocked_by:
                lines.append(f"- **Blocked by:** `{blocked}`")

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

    if e.secure_raw is not None and e.secure_raw != e.secure_adjusted:
        raw = "PASS" if e.secure_raw else "FAIL"
        adjusted = "PASS" if e.secure_adjusted else "FAIL"
        lines.append("")
        lines.append(f"**SECURE overlay:** {raw} (raw) -> {adjusted} (acknowledged)")
    if e.acknowledged_risks:
        lines.append("")
        lines.append("## Acknowledged Risks")
        for risk in e.acknowledged_risks:
            lines.append(
                f"- `{risk.callee or risk.kind}` line {risk.line}: {risk.reason}"
            )
    if e.grade_capped:
        lines.append(
            "> Max grade capped below IDEAL because an acknowledged security "
            "risk is active."
        )

    # Failing complexity gates point at concrete edit targets — show them by
    # default so an agent never has to guess where the offending function is.
    if e.metric_locations:
        lines.append("")
        lines.append("## Metric Locations")
        for metric, entries in e.metric_locations.items():
            lines.append(f"- `{metric}`:")
            for fn in entries:
                where = (
                    "module-level (not attributable to a function)"
                    if fn.kind == "module"
                    else f"`{fn.qualified_name or fn.name}` "
                    f"({fn.kind}) lines {fn.start_line}-{fn.end_line}"
                )
                lines.append(f"  - {where} — complexity {fn.complexity}")

    if e.refactor_targets:
        lines.append("")
        lines.append("## Refactor Targets")
        lines.append("| Target | Kind | Metric | Location | Operations |")
        lines.append("| --- | --- | --- | --- | --- |")
        for target in e.refactor_targets:
            loc = (
                f"{target.line_start}-{target.line_end}"
                if target.line_end and target.line_end != target.line_start
                else str(target.line_start or "?")
            )
            symbol = (target.symbol or "<module>").replace("|", "\\|")
            ops = ", ".join(f"`{op}`" for op in target.recommended_operations)
            lines.append(
                f"| `{target.target_id}` `{symbol}` | {target.kind} | "
                f"`{target.metric}` | {loc} | {ops} |"
            )

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
