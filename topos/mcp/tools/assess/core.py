"""
Assessment tool — compare current vs. proposed code on the lattice.

This is the main tool for agent refactor loops. When ``filepath`` is provided,
the baseline is evaluated against the cached ``ModuleDependencyGraph`` and the
proposed AST is scored against that same graph (approximating coupling under
the refactor). Anti-gaming guardrail: if scores moved meaningfully while AST
edit distance is near zero, status becomes ``SUSPICIOUS_NO_STRUCTURAL_CHANGE``.
"""

from __future__ import annotations

import hashlib
from pathlib import Path

from fastmcp.tools.base import ToolResult

from topos.core.morphism import ProgramMorphism
from topos.evaluation.characteristic_morphism import (
    CharacteristicMorphism,
    ClassificationResult,
)
from topos.evaluation.policies.base import Priority
from topos.functors.profunctors.ast.compare import calculate_ast_distance

from ...diagnostics import overlay_for_source
from ...evaluation import (
    classify_morphism,
    detect_language,
    gitnexus_warnings,
    load_dep_graph,
    resolve_gitnexus_dir,
)
from ...formatting import to_evaluation_result, to_tool_result
from ...schemas import (
    AgentContract,
    AssessImprovementInput,
    AssessmentResult,
    AssessmentStatus,
    EvaluationResult,
    LatticeElement,
    PrioritySource,
    resolve_priority,
)
from ...security import (
    read_safe_utf8_file,
    resolve_file_root,
    resolve_within_root,
)
from ...server import mcp
from .render import _REGRESSION_STATUSES, _regression_diff, render_assessment_md

_READ_ONLY_ANN = {
    "title": "Topos Refactor Assessment",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}

_STRUCTURAL_CHANGE_THRESHOLD = 0.02
_MEANINGFUL_SCORE_DELTA = 3.0


def _load_baseline(
    params: AssessImprovementInput,
) -> tuple[str, bool, list[str], object | None]:
    if params.filepath:
        resolved, err = resolve_within_root(params.filepath)
        if err or resolved is None:
            raise ValueError((err or {}).get("error", "path error"))
        if not resolved.is_file():
            raise ValueError(f"Path is not a file: {resolved}")
        current_src, read_err = read_safe_utf8_file(resolved)
        if read_err or current_src is None:
            raise ValueError((read_err or {}).get("error", "read error"))
        project_root = resolve_file_root()
        gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir, project_root)
        dep_graph = load_dep_graph(gitnexus_dir, str(resolved))
        warnings = gitnexus_warnings(
            params.gitnexus_dir,
            project_root,
            gitnexus_dir,
            dep_graph_loaded=dep_graph is not None,
        )
        return current_src, dep_graph is not None, warnings, dep_graph
    elif params.current_code:
        warnings = [
            "COMPOSABLE not scored — current_code mode has no filepath or "
            "ModuleDependencyGraph context."
        ]
        return params.current_code, False, warnings, None
    else:
        raise ValueError("Provide either `filepath` or `current_code`.")


def _is_suspicious(
    status: AssessmentStatus, distance: float | None, score_deltas: dict[str, float]
) -> bool:
    if distance is None:
        return False
    if distance >= _STRUCTURAL_CHANGE_THRESHOLD:
        return False
    if status not in (AssessmentStatus.IMPROVEMENT, AssessmentStatus.IMPROVEMENT_SCORE):
        return False
    return any(abs(d) >= _MEANINGFUL_SCORE_DELTA for d in score_deltas.values())


def _determine_lattice_status(
    cur_summary, prop_summary, score_deltas
) -> AssessmentStatus:
    lattice = CharacteristicMorphism().omega
    if cur_summary == prop_summary:
        score_improved = any(d > 0 for d in score_deltas.values())
        score_regressed = any(d < 0 for d in score_deltas.values())
        if score_improved and not score_regressed:
            return AssessmentStatus.IMPROVEMENT_SCORE
        if score_regressed and not score_improved:
            return AssessmentStatus.REGRESSION_SCORE
        return AssessmentStatus.LATERAL_MOVE

    if lattice.leq(cur_summary, prop_summary):
        return AssessmentStatus.IMPROVEMENT
    if lattice.leq(prop_summary, cur_summary):
        return AssessmentStatus.REGRESSION
    return AssessmentStatus.LATERAL_MOVE


def _determine_assessment_status(
    current_res, proposed_res, score_deltas, distance
) -> tuple[AssessmentStatus, str | None]:
    cur_summary = current_res.summary()
    prop_summary = proposed_res.summary()
    status = _determine_lattice_status(cur_summary, prop_summary, score_deltas)

    suspicion = None
    if _is_suspicious(status, distance, score_deltas):
        status = AssessmentStatus.SUSPICIOUS_NO_STRUCTURAL_CHANGE
        suspicion = (
            f"Scores improved (deltas={score_deltas}) but normalized AST edit "
            f"distance is only {distance:.3f} — the tree barely changed. Either "
            "the refactor is trivially cosmetic (comment/whitespace shuffle) "
            "or the scoring is oscillating. Re-verify with a concrete "
            "structural change."
        )
    return status, suspicion


def _evaluate_proposed(
    proposed_src: str,
    dep_graph,
    priority: Priority,
    language: str,
) -> tuple[ClassificationResult, ProgramMorphism]:
    proposed_morph = ProgramMorphism(source=proposed_src, language=language)
    proposed_res = classify_morphism(proposed_morph, priority, dep_graph)
    return proposed_res, proposed_morph


def _calculate_deltas(
    current_eval: EvaluationResult,
    proposed_eval: EvaluationResult,
    current_res: ClassificationResult,
    proposed_res: ClassificationResult,
) -> tuple[dict[str, float], dict[str, float]]:
    all_dims = set(current_eval.scores) | set(proposed_eval.scores)
    score_deltas = {
        dim: round(
            proposed_eval.scores.get(dim, 0.0) - current_eval.scores.get(dim, 0.0), 1
        )
        for dim in all_dims
    }

    all_metrics = set(current_res.raw_metrics) | set(proposed_res.raw_metrics)
    metric_deltas = {
        m: round(
            proposed_res.raw_metrics.get(m, 0.0) - current_res.raw_metrics.get(m, 0.0),
            3,
        )
        for m in all_metrics
    }
    return score_deltas, metric_deltas


@mcp.tool(
    name="topos_assess_improvement",
    tags={"assess", "workflow"},
    annotations=_READ_ONLY_ANN,
)
def topos_assess_improvement(params: AssessImprovementInput) -> ToolResult:
    """Compare proposed code against the current baseline.

    **Preferred usage** — pass ``filepath`` (code loaded from disk + coupling
    scored against the cached ``ModuleDependencyGraph``). The proposed code is
    parsed, but coupling is an approximation: it uses the *current* dep graph
    for the target file, so inbound edges from other files reflect the
    pre-refactor state. That's fine for tight iteration loops.

    **Legacy usage** — pass ``current_code`` + ``proposed_code``. Coupling is
    NOT computed (AST-only).

    For edit-in-place loops, prefer ``topos_assess_worktree_change`` (compare
    against a git ref) or ``topos_begin_refactor`` + ``topos_assess_snapshot``.

    Anti-gaming: when scores move meaningfully but AST edit distance is near
    zero, status becomes ``SUSPICIOUS_NO_STRUCTURAL_CHANGE`` and
    ``suspicion_reason`` is populated.
    """
    priority, priority_source = resolve_priority(params.preferences)
    try:
        baseline_src, coupling_for_proposed, warnings, dep_graph = _load_baseline(
            params
        )
    except ValueError as exc:
        return _err_assessment(priority, priority_source, str(exc))

    proposed_src, proposed_err = _load_proposed_source(params)
    if proposed_err or proposed_src is None:
        return _err_assessment(
            priority, priority_source, proposed_err or "Unable to load proposed source."
        )

    prefs = params.preferences.to_preferences() if params.preferences else None
    file_path = None
    if params.filepath:
        file_path, _ = resolve_within_root(params.filepath)

    model = _assess_core(
        baseline_src=baseline_src,
        proposed_src=proposed_src,
        language=params.language,
        priority=priority,
        priority_source=priority_source,
        prefs=prefs,
        dep_graph=dep_graph,
        coupling_for_proposed=coupling_for_proposed,
        file_path=file_path,
        allow=params.allow,
        include_security_findings=params.include_security_findings,
        warnings=warnings,
    )
    return to_tool_result(model, render_assessment_md(model))


def _assess_core(
    *,
    baseline_src: str,
    proposed_src: str,
    language: str,
    priority: Priority,
    priority_source: PrioritySource,
    prefs,
    dep_graph,
    coupling_for_proposed: bool,
    file_path,
    allow: list[str],
    include_security_findings: bool,
    warnings: list[str],
) -> AssessmentResult:
    """Score a baseline vs. a proposed source and classify the move on Ω."""
    baseline_morph = ProgramMorphism(source=baseline_src, language=language)
    baseline_res = classify_morphism(baseline_morph, priority, dep_graph)
    proposed_res, proposed_morph = _evaluate_proposed(
        proposed_src, dep_graph, priority, language
    )

    current_overlay = overlay_for_source(
        baseline_src,
        language,
        baseline_res,
        file_path=file_path,
        allows=allow,
        include_security_findings=include_security_findings,
    )
    proposed_overlay = overlay_for_source(
        proposed_src,
        language,
        proposed_res,
        file_path=file_path,
        allows=allow,
        include_security_findings=include_security_findings,
    )
    current_eval = to_evaluation_result(
        baseline_res,
        coupling_available=dep_graph is not None,
        preferences=prefs,
        priority_source=priority_source,
        include_agent_contract=False,
        **_overlay_kwargs(current_overlay),
    )
    proposed_eval = to_evaluation_result(
        proposed_res,
        coupling_available=coupling_for_proposed,
        preferences=prefs,
        priority_source=priority_source,
        include_agent_contract=False,
        **_overlay_kwargs(proposed_overlay),
    )

    score_deltas, metric_deltas = _calculate_deltas(
        current_eval, proposed_eval, baseline_res, proposed_res
    )

    distance = None
    similarity = None
    if baseline_res.is_parseable and proposed_res.is_parseable:
        dist = calculate_ast_distance(baseline_morph.ast, proposed_morph.ast)
        distance = dist.normalized_distance
        similarity = 1.0 - dist.normalized_distance

    status, suspicion = _determine_assessment_status(
        baseline_res, proposed_res, score_deltas, distance
    )

    regression_diff = None
    if status in _REGRESSION_STATUSES:
        regression_diff = _regression_diff(baseline_src, proposed_src, language)

    return AssessmentResult(
        status=status,
        priority=priority,
        priority_source=priority_source,
        current=current_eval,
        proposed=proposed_eval,
        score_deltas=score_deltas,
        metric_deltas=metric_deltas,
        structural_distance=distance,
        similarity=similarity,
        coupling_available_for_proposed=coupling_for_proposed,
        baseline_hash=hashlib.sha256(baseline_src.encode("utf-8")).hexdigest(),
        current_hash=hashlib.sha256(proposed_src.encode("utf-8")).hexdigest(),
        warnings=warnings,
        agent_contract=_assessment_contract(status, warnings, proposed_eval),
        suspicion_reason=suspicion,
        regression_diff=regression_diff,
    )


def _assess_edit_in_place(
    *,
    baseline_src: str,
    resolved_path: Path,
    gitnexus_dir_override: str | None,
    priority: Priority,
    priority_source: PrioritySource,
    prefs,
    allow: list[str],
    include_security_findings: bool,
    extra_warnings: list[str],
) -> ToolResult:
    """Read the current on-disk file and assess it against ``baseline_src``."""
    current_src, read_err = read_safe_utf8_file(resolved_path)
    if read_err or current_src is None:
        return _err_assessment(
            priority,
            priority_source,
            (read_err or {}).get("error", "read error"),
            blocked_by="file_not_found",
        )
    project_root = resolve_file_root()
    gitnexus_dir = resolve_gitnexus_dir(gitnexus_dir_override, project_root)
    dep_graph = load_dep_graph(gitnexus_dir, str(resolved_path))
    warnings = [
        *extra_warnings,
        *gitnexus_warnings(
            gitnexus_dir_override,
            project_root,
            gitnexus_dir,
            dep_graph_loaded=dep_graph is not None,
        ),
    ]
    model = _assess_core(
        baseline_src=baseline_src,
        proposed_src=current_src,
        language=detect_language(resolved_path),
        priority=priority,
        priority_source=priority_source,
        prefs=prefs,
        dep_graph=dep_graph,
        coupling_for_proposed=dep_graph is not None,
        file_path=resolved_path,
        allow=allow,
        include_security_findings=include_security_findings,
        warnings=warnings,
    )
    return to_tool_result(model, render_assessment_md(model))


def _err_assessment(
    priority: Priority,
    priority_source: PrioritySource,
    msg: str,
    *,
    blocked_by: str = "assessment_error",
) -> ToolResult:
    empty = EvaluationResult(
        is_parseable=False,
        lattice_element=LatticeElement.SLOP,
        lattice_symbol="⊥",
        lattice_description="not evaluated",
        dimensions={},
        scores={},
        priority=priority,
        priority_source=priority_source,
        guidance="",
        coupling_available=False,
    )
    model = AssessmentResult(
        status=AssessmentStatus.LATERAL_MOVE,
        priority=priority,
        priority_source=priority_source,
        current=empty,
        proposed=empty,
        score_deltas={},
        structural_distance=None,
        similarity=None,
        coupling_available_for_proposed=False,
        agent_contract=AgentContract(
            blocked_by=[blocked_by],
            risk_flags=[blocked_by],
        ),
        error=msg,
    )
    return to_tool_result(model, render_assessment_md(model))


def _load_proposed_source(
    params: AssessImprovementInput,
) -> tuple[str | None, str | None]:
    if params.proposed_code is not None:
        return params.proposed_code, None
    if params.proposed_filepath is None:
        return None, "Provide exactly one of `proposed_code` or `proposed_filepath`."
    source, err = read_safe_utf8_file(params.proposed_filepath)
    if err:
        return None, err["error"]
    return source, None


def _overlay_kwargs(overlay):
    if overlay is None:
        return {}
    return {
        "security_findings": overlay.active_findings,
        "acknowledged_risks": overlay.acknowledged_risks,
        "adjusted_verdict": overlay.verdict,
    }


def _assessment_contract(
    status: AssessmentStatus,
    warnings: list[str],
    proposed_eval: EvaluationResult,
) -> AgentContract:
    risk_flags: list[str] = []
    blocked_by: list[str] = []
    next_actions: list[str] = []

    if warnings:
        risk_flags.append("warnings")
    if proposed_eval.grade_capped:
        risk_flags.append("grade_capped")
    if proposed_eval.security_findings:
        risk_flags.append("active_security_findings")

    if status == AssessmentStatus.SUSPICIOUS_NO_STRUCTURAL_CHANGE:
        blocked_by.append("suspicious_no_structural_change")
        risk_flags.append("metric_gaming_risk")
        next_tool = "topos_inspect_code"
        next_actions.append("make a real structural change before reassessing")
    elif status in _REGRESSION_STATUSES:
        blocked_by.append("regression")
        risk_flags.append("regression")
        next_tool = "topos_inspect_code"
        next_actions.append("discard or revise the proposed change")
    elif status in (AssessmentStatus.IMPROVEMENT, AssessmentStatus.IMPROVEMENT_SCORE):
        next_tool = "topos_evaluate_project"
        next_actions.append("run project rollup and behavior checks before accepting")
    else:
        next_tool = "topos_inspect_code"
        next_actions.append("try a different focused structural change")

    return AgentContract(
        next_tool=next_tool,
        next_actions=next_actions,
        blocked_by=blocked_by,
        verification_gates=[
            "assessment status is IMPROVEMENT or IMPROVEMENT_SCORE",
            "assessment status is not SUSPICIOUS_NO_STRUCTURAL_CHANGE",
            "behavior tests or type/lint checks pass when available",
        ],
        risk_flags=risk_flags,
    )
