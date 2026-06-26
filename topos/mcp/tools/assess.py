"""
Assessment tool — compare current vs. proposed code on the lattice.

This is the main tool for agent refactor loops. When ``filepath`` is provided,
the baseline is evaluated against the cached ``ModuleDependencyGraph`` and the
proposed AST is scored against that same graph (approximating coupling under
the refactor). Anti-gaming guardrail: if scores moved meaningfully while AST
edit distance is near zero, status becomes ``SUSPICIOUS_NO_STRUCTURAL_CHANGE``.
"""

from __future__ import annotations

import difflib
import hashlib
import subprocess
from pathlib import Path

from fastmcp.tools.base import ToolResult

from topos.core.morphism import ProgramMorphism
from topos.evaluation.characteristic_morphism import (
    CharacteristicMorphism,
    ClassificationResult,
)
from topos.evaluation.policies.base import Priority
from topos.functors.probes.cfg.complexity import cyclomatic_complexity
from topos.functors.profunctors.ast.compare import calculate_ast_distance
from topos.graphs.cfg.builder import _collect_callables, build_cfg_from_uast
from topos.graphs.cfg.object import ControlFlowGraph
from topos.utils.discovery import find_git_root

from ..diagnostics import overlay_for_source
from ..evaluation import (
    classify_morphism,
    detect_language,
    gitnexus_warnings,
    load_dep_graph,
    resolve_gitnexus_dir,
)
from ..formatting import to_evaluation_result, to_tool_result
from ..schemas import (
    AgentContract,
    AssessImprovementInput,
    AssessmentResult,
    AssessmentStatus,
    AssessSnapshotInput,
    AssessWorktreeChangeInput,
    BeginRefactorInput,
    EvaluationResult,
    LatticeElement,
    PrioritySource,
    SnapshotResult,
    resolve_priority,
)
from ..security import (
    read_safe_utf8_file,
    resolve_file_root,
    resolve_within_root,
)
from ..server import mcp
from ..snapshots import now as snapshot_now
from ..snapshots import read_snapshot, write_snapshot

_READ_ONLY_ANN = {
    "title": "Topos Refactor Assessment",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}

# Near-zero edit distance threshold for gaming detection.
_STRUCTURAL_CHANGE_THRESHOLD = 0.02  # normalized distance
_MEANINGFUL_SCORE_DELTA = 3.0  # percentage points

# Cap the function-scoped regression diff so it stays a pinpoint, not a dump.
_REGRESSION_DIFF_MAX_LINES = 40


def _load_baseline(
    params: AssessImprovementInput,
) -> tuple[str, bool, list[str], object | None]:
    """Resolve the baseline source + coupling context for ``assess_improvement``.

    Returns ``(baseline_src, coupling_for_proposed, warnings, dep_graph)``;
    classification is deferred to ``_assess_core`` so both sides score against
    the same dep graph.
    """
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
    """Score a baseline vs. a proposed source and classify the move on Ω.

    The single comparison engine behind every assessment entry point
    (``topos_assess_improvement``, ``topos_assess_worktree_change``,
    ``topos_assess_snapshot``) so they share identical status semantics. Both
    sides are scored against ``dep_graph`` when present, so COMPOSABLE is
    available in snapshot/worktree modes too.
    """
    # ---- classify both sides against the same dep graph ----
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
    # Warnings live on the top-level AssessmentResult only; the nested
    # current/proposed evals would otherwise duplicate the identical list.
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

    # ---- score & metric deltas ----
    score_deltas, metric_deltas = _calculate_deltas(
        current_eval, proposed_eval, baseline_res, proposed_res
    )

    # ---- structural distance ----
    distance = None
    similarity = None
    if baseline_res.is_parseable and proposed_res.is_parseable:
        dist = calculate_ast_distance(baseline_morph.ast, proposed_morph.ast)
        distance = dist.normalized_distance
        similarity = 1.0 - dist.normalized_distance

    # ---- status classification & anti-gaming ----
    status, suspicion = _determine_assessment_status(
        baseline_res, proposed_res, score_deltas, distance
    )

    # ---- regression pinpoint ----
    # On a regression/suspicious verdict, give the agent a function-scoped diff
    # of the single worst function instead of forcing a full metric-tree diff.
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


# ---------------------------------------------------------------------------
# Edit-in-place assessment — snapshot + git-worktree entry points
#
# Both recover the "before" source the agent obliterated by editing in place,
# then run the same ``_assess_core``. The worktree path is stateless (git);
# the snapshot path uses the content-addressed store in ``..snapshots``.
# ---------------------------------------------------------------------------

_WRITE_ANN = {
    "title": "Topos Begin Refactor",
    "readOnlyHint": False,  # writes a baseline snapshot to scratch storage
    "destructiveHint": False,
    "idempotentHint": True,  # content-addressed: re-capturing same source is a noop
    "openWorldHint": False,
}


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


def _git_show(
    repo_root: Path, ref: str, rel_path: str
) -> tuple[str | None, str | None]:
    """Read ``<ref>:<rel_path>`` from git, mirroring discovery.py's git idiom.

    Returns ``(source, None)`` or ``(None, blocked_by_code)``. ``git_unavailable``
    when git is not installed; ``baseline_ref_not_found`` when the ref or path
    does not exist at that revision (also covers timeouts/OS errors).
    """
    try:
        result = subprocess.run(
            ["git", "-C", str(repo_root), "show", f"{ref}:{rel_path}"],
            capture_output=True,
            timeout=5,
        )
    except FileNotFoundError:
        return None, "git_unavailable"
    except (OSError, subprocess.TimeoutExpired):
        return None, "baseline_ref_not_found"
    if result.returncode != 0:
        return None, "baseline_ref_not_found"
    return result.stdout.decode("utf-8", errors="replace"), None


@mcp.tool(
    name="topos_assess_worktree_change",
    tags={"assess", "workflow"},
    annotations=_READ_ONLY_ANN,
)
def topos_assess_worktree_change(params: AssessWorktreeChangeInput) -> ToolResult:
    """Assess an in-place edit against a git revision — the common refactor loop.

    Stateless: the baseline is read from git (``git show <baseline_ref>:<path>``,
    default ``HEAD``) and compared to the current working-tree file. No prior
    call required — edit the file, then ask "did it beat HEAD?". COMPOSABLE is
    scored when a ``.gitnexus`` dep graph is available.

    For untracked/new files or an uncommitted pre-edit baseline (which git
    cannot serve), use ``topos_begin_refactor`` + ``topos_assess_snapshot``.
    """
    priority, priority_source = resolve_priority(params.preferences)
    resolved, err = resolve_within_root(params.filepath)
    if err or resolved is None:
        return _err_assessment(
            priority, priority_source, (err or {}).get("error", "path error")
        )

    git_root = find_git_root(resolved)
    if git_root is None:
        return _err_assessment(
            priority,
            priority_source,
            f"Not inside a git repository: {resolved}",
            blocked_by="not_a_git_repo",
        )
    rel_path = resolved.relative_to(git_root).as_posix()
    baseline_src, git_err = _git_show(git_root, params.baseline_ref, rel_path)
    if git_err or baseline_src is None:
        return _err_assessment(
            priority,
            priority_source,
            f"Could not read `{rel_path}` at ref `{params.baseline_ref}`.",
            blocked_by=git_err or "baseline_ref_not_found",
        )

    prefs = params.preferences.to_preferences() if params.preferences else None
    return _assess_edit_in_place(
        baseline_src=baseline_src,
        resolved_path=resolved,
        gitnexus_dir_override=params.gitnexus_dir,
        priority=priority,
        priority_source=priority_source,
        prefs=prefs,
        allow=params.allow,
        include_security_findings=params.include_security_findings,
        extra_warnings=[],
    )


@mcp.tool(
    name="topos_begin_refactor",
    tags={"assess", "workflow"},
    annotations=_WRITE_ANN,
)
def topos_begin_refactor(params: BeginRefactorInput) -> ToolResult:
    """Capture a file's current source as a baseline snapshot before editing.

    Returns a ``snapshot_id``. Edit the file in place, then call
    ``topos_assess_snapshot(snapshot_id, filepath)`` to score the change — no
    need to re-send the baseline. Use this when the baseline is not a committed
    git revision (untracked/new files, or uncommitted prior edits); otherwise
    ``topos_assess_worktree_change`` needs no snapshot at all.
    """
    resolved, err = resolve_within_root(params.filepath)
    if err or resolved is None:
        return _err_snapshot(params.filepath, (err or {}).get("error", "path error"))
    if not resolved.is_file():
        return _err_snapshot(params.filepath, f"Path is not a file: {resolved}")
    baseline_src, read_err = read_safe_utf8_file(resolved)
    if read_err or baseline_src is None:
        return _err_snapshot(
            params.filepath, (read_err or {}).get("error", "read error")
        )

    priority, _ = resolve_priority(params.preferences)
    meta = {
        "filepath": str(resolved),
        "priority": priority.value,
        "ranking": (
            [g.value for g in params.preferences.ranking]
            if params.preferences
            else None
        ),
        "target": (
            params.preferences.target.value
            if params.preferences and params.preferences.target
            else None
        ),
        "gitnexus_dir": params.gitnexus_dir,
    }
    created_at = snapshot_now()
    project_root = resolve_file_root()
    snapshot_id = write_snapshot(
        project_root, baseline_src, meta, created_at=created_at
    )
    model = SnapshotResult(
        snapshot_id=snapshot_id,
        filepath=str(resolved),
        baseline_hash=hashlib.sha256(baseline_src.encode("utf-8")).hexdigest(),
        created_at=created_at,
        agent_contract=AgentContract(
            next_tool="topos_assess_snapshot",
            next_actions=[
                "edit the file in place, then call topos_assess_snapshot with this "
                "snapshot_id"
            ],
        ),
    )
    return to_tool_result(model, _render_snapshot_md(model))


@mcp.tool(
    name="topos_assess_snapshot",
    tags={"assess", "workflow"},
    annotations=_READ_ONLY_ANN,
)
def topos_assess_snapshot(params: AssessSnapshotInput) -> ToolResult:
    """Assess the current file against a baseline captured by topos_begin_refactor.

    Loads the stored baseline by ``snapshot_id`` and compares it to the current
    on-disk file, with the same status semantics as ``topos_assess_improvement``
    (and COMPOSABLE scored when a dep graph is available). A missing or expired
    snapshot is reported via ``blocked_by`` (``snapshot_not_found`` /
    ``snapshot_stale``).
    """
    resolved, err = resolve_within_root(params.filepath)
    if err or resolved is None:
        return _err_assessment(
            Priority.SIMPLE,
            PrioritySource.DEFAULT,
            (err or {}).get("error", "path error"),
        )

    project_root = resolve_file_root()
    load = read_snapshot(project_root, params.snapshot_id, at=snapshot_now())
    if load.blocked_by or load.meta is None or load.baseline_src is None:
        return _err_assessment(
            Priority.SIMPLE,
            PrioritySource.DEFAULT,
            f"Snapshot `{params.snapshot_id}` is unavailable.",
            blocked_by=load.blocked_by or "snapshot_not_found",
        )
    # A snapshot is bound to the file it was taken from; a different path means
    # the agent is reusing a stale id.
    if load.meta.get("filepath") != str(resolved):
        return _err_assessment(
            Priority.SIMPLE,
            PrioritySource.DEFAULT,
            (
                f"Snapshot was taken from `{load.meta.get('filepath')}`, not "
                f"`{resolved}`."
            ),
            blocked_by="snapshot_stale",
        )

    priority, priority_source, prefs = _priority_from_meta(load.meta)
    return _assess_edit_in_place(
        baseline_src=load.baseline_src,
        resolved_path=resolved,
        gitnexus_dir_override=load.meta.get("gitnexus_dir"),
        priority=priority,
        priority_source=priority_source,
        prefs=prefs,
        allow=params.allow,
        include_security_findings=params.include_security_findings,
        extra_warnings=[],
    )


def _priority_from_meta(meta: dict):
    """Reconstruct (priority, priority_source, preferences) from a snapshot sidecar.

    Rebuilds a ``UserPreferencesInput`` so priority/preferences resolution reuses
    the same validated logic as a live tool call rather than duplicating it.
    """
    ranking = meta.get("ranking")
    if not ranking:
        return (
            Priority(meta.get("priority", Priority.SIMPLE.value)),
            (PrioritySource.DEFAULT),
            None,
        )
    from topos.evaluation.preferences import Generator

    from ..schemas import UserPreferencesInput

    target = meta.get("target")
    prefs_input = UserPreferencesInput(
        ranking=[Generator(r) for r in ranking],
        target=LatticeElement(target) if target else None,
    )
    return (
        prefs_input.to_priority(),
        PrioritySource.PREFERENCES,
        prefs_input.to_preferences(),
    )


def _err_snapshot(filepath: str, msg: str) -> ToolResult:
    model = SnapshotResult(
        snapshot_id="",
        filepath=filepath,
        baseline_hash="",
        created_at=0.0,
        agent_contract=AgentContract(
            blocked_by=["snapshot_error"], risk_flags=["snapshot_error"]
        ),
        error=msg,
    )
    return to_tool_result(model, _render_snapshot_md(model))


def _render_snapshot_md(r: SnapshotResult) -> str:
    if r.error:
        return f"**Error:** {r.error}"
    lines = [
        f"**Snapshot captured:** `{r.snapshot_id}`",
        f"**File:** `{r.filepath}`",
        "",
        "Edit the file in place, then call "
        f'`topos_assess_snapshot(snapshot_id="{r.snapshot_id}", '
        f'filepath="{r.filepath}")`.',
    ]
    return "\n".join(lines)


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


# ---------------------------------------------------------------------------
# Regression pinpoint — function-scoped unified diff
# ---------------------------------------------------------------------------

# Statuses that warrant a targeted regression diff.
_REGRESSION_STATUSES = frozenset(
    {
        AssessmentStatus.REGRESSION,
        AssessmentStatus.REGRESSION_SCORE,
        AssessmentStatus.SUSPICIOUS_NO_STRUCTURAL_CHANGE,
    }
)


def _span_text(source_bytes: bytes, span) -> str:
    """Slice a UAST byte span out of the UTF-8-encoded source.

    UAST offsets are byte offsets, so we must index the encoded bytes, not the
    code-point-indexed str. Bounds-guarded like ``cpg/object.py`` in case the
    span refers to a different revision than ``source_bytes``.
    """
    if span.end_byte > len(source_bytes):
        return ""
    return source_bytes[span.start_byte : span.end_byte].decode(
        "utf-8", errors="replace"
    )


def _function_complexities(
    source: str, language: str
) -> dict[str, tuple[int, list[str]]]:
    """Map function name -> (cyclomatic_complexity, source_lines).

    Mirrors the callable-collection pattern in ``inspect.py``. Source lines are
    sliced by the UAST byte span so they round-trip exactly into difflib.
    """
    out: dict[str, tuple[int, list[str]]] = {}
    morph = ProgramMorphism(source=source, language=language)
    if not (morph.ast and morph.ast.uast_root):
        return out
    # UAST spans are UTF-8 byte offsets; encode ONCE and slice the bytes so
    # non-ASCII source (→, —, emoji) doesn't shift names/bodies. See _span_text.
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
            # Overloads / duplicate names: skip rather than guess which moved.
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
        # No keepends: difflib + lineterm="" then a "\n".join keeps lines clean.
        out[name] = (complexity, body.splitlines())
    return out


def _regression_diff(current_src: str, proposed_src: str, language: str) -> str | None:
    """Unified diff of the single function with the worst complexity increase.

    Returns ``None`` (rather than a whole-file diff) when no function got more
    complex, or when function matching is ambiguous — keeps the output lean and
    actionable. stdlib ``difflib`` only.
    """
    cur = _function_complexities(current_src, language)
    prop = _function_complexities(proposed_src, language)
    if not cur or not prop:
        return None

    # Match by name; find the largest ADVERSE complexity increase.
    worst_name: str | None = None
    worst_delta = 0
    for name, (prop_cx, _) in prop.items():
        if name not in cur:
            # Rename/add — don't dump a whole-function diff. Fallback: None.
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
    return "\n".join([header, *body])


# ---------------------------------------------------------------------------
# Markdown renderer (rendered into ToolResult.content)
# ---------------------------------------------------------------------------

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
    """Compact markdown for a refactor assessment.

    Summarizes current vs. proposed rather than dumping both full evaluations;
    the structured_content channel still carries everything.
    """
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
    return "\n".join(lines)
