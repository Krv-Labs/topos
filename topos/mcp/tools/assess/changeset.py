"""Changeset assessment — multi-file refactors and module splits (issue #68).

``topos_assess_improvement`` is single-file. Splitting a large module into
smaller ones needs a changeset view: per-file before/after verdicts, a project
rollup, and a flag when function complexity merely *relocated* inside the same
file instead of moving across a module boundary.
"""

from __future__ import annotations

from pathlib import Path

from fastmcp.tools.base import ToolResult

from topos.core.omega import EvaluationValue, verdict_from_generators
from topos.evaluation.policies.base import Priority
from topos.utils.discovery import find_git_root

from ...evaluation import (
    detect_language,
    gitnexus_warnings,
    load_dep_graph,
    resolve_gitnexus_dir,
)
from ...formatting import lattice_to_str, to_tool_result
from ...schemas import (
    AgentContract,
    AssessChangesetInput,
    ChangesetFileEntry,
    ChangesetResult,
    EvaluationResult,
    LatticeElement,
    PrioritySource,
    resolve_priority,
)
from ...security import read_safe_utf8_file, resolve_file_root, resolve_within_root
from ...server import mcp
from .core import _assess_core
from .worktree import _git_show, _ref_exists

# Read-only: reads each file from git and the working tree, scores in memory,
# writes nothing. When COMPOSABLE is blocked the contract points next_tool at
# topos_generate_depgraph (approval-gated) — refresh happens there, not here.
_READ_ONLY_ANN = {
    "title": "Topos Changeset Assessment",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}

_DIM_VALUE = {
    "simple": EvaluationValue.SIMPLE,
    "composable": EvaluationValue.COMPOSABLE,
    "secure": EvaluationValue.SECURE,
}


@mcp.tool(
    name="topos_assess_changeset",
    tags={"assess", "workflow"},
    annotations=_READ_ONLY_ANN,
)
def topos_assess_changeset(params: AssessChangesetInput) -> ToolResult:
    """Assess a multi-file changeset against a git baseline and roll the
    per-file verdicts into a project before/after (read-only).

    Use for a module split or any edit spanning several files; for a single
    file use ``topos_assess_worktree_change``. Each file is compared to
    ``baseline_ref`` (new files have no baseline). Flags
    ``complexity_relocated_within_file`` when a function shrank but its file's
    cyclomatic complexity grew, and ``project_regression`` when the rollup drops
    a generator it previously satisfied. Reads from git and the working tree and
    writes nothing. When COMPOSABLE is blocked (``missing_gitnexus_dir`` /
    ``stale_gitnexus_dir`` in the contract), call ``topos_generate_depgraph``
    first, then re-assess. Returns a ChangesetResult.
    """
    priority, priority_source = resolve_priority(params.preferences)
    prefs = params.preferences.to_preferences() if params.preferences else None
    project_root = resolve_file_root()

    # Validate the baseline ref once, up front: git cannot tell a genuinely-new
    # file apart from a mistyped ref (both are "absent at ref"). Without this a
    # bad ref would silently mark every file is_new and report a green result.
    git_root = find_git_root(project_root)
    if git_root is not None and not _ref_exists(git_root, params.baseline_ref):
        return _changeset_error(
            priority,
            priority_source,
            params.baseline_ref,
            f"baseline ref not found: {params.baseline_ref}",
        )

    gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir, project_root)

    entries: list[ChangesetFileEntry] = []
    before_evals: list[EvaluationResult] = []
    after_evals: list[EvaluationResult] = []
    any_coupling = False

    for filepath in params.files:
        outcome = _assess_one_file(
            filepath,
            params,
            priority,
            priority_source,
            prefs,
            project_root,
            gitnexus_dir,
        )
        if outcome.git_unavailable:
            return _changeset_error(
                priority, priority_source, params.baseline_ref, "git is not available."
            )
        entries.append(outcome.entry)
        if outcome.coupling:
            any_coupling = True
        if outcome.baseline_eval is not None:
            before_evals.append(outcome.baseline_eval)
        if outcome.current_eval is not None:
            after_evals.append(outcome.current_eval)

    return _build_changeset_result(
        params=params,
        priority=priority,
        priority_source=priority_source,
        entries=entries,
        before_evals=before_evals,
        after_evals=after_evals,
        coupling_available=any_coupling,
    )


class _FileOutcome:
    __slots__ = (
        "entry",
        "baseline_eval",
        "current_eval",
        "coupling",
        "git_unavailable",
    )

    def __init__(self, entry, baseline_eval, current_eval, coupling, git_unavailable):
        self.entry = entry
        self.baseline_eval = baseline_eval
        self.current_eval = current_eval
        self.coupling = coupling
        self.git_unavailable = git_unavailable


def _assess_one_file(
    filepath: str,
    params: AssessChangesetInput,
    priority: Priority,
    priority_source: PrioritySource,
    prefs,
    project_root: Path,
    gitnexus_dir,
) -> _FileOutcome:
    resolved, err = resolve_within_root(filepath)
    if err or resolved is None:
        return _file_error(filepath, (err or {}).get("error", "path error"))

    git_root = find_git_root(resolved)
    if git_root is None:
        return _file_error(filepath, "not inside a git repo", code="not_a_git_repo")

    rel_path = resolved.relative_to(git_root).as_posix()
    baseline_src, git_err = _git_show(git_root, params.baseline_ref, rel_path)
    if git_err == "git_unavailable":
        return _FileOutcome(None, None, None, False, True)
    is_new = git_err == "baseline_ref_not_found"
    if baseline_src is None:
        baseline_src = ""

    current_src, read_err = read_safe_utf8_file(resolved)
    if read_err or current_src is None:
        return _file_error(
            filepath, (read_err or {}).get("error", "read error"), code="file_not_found"
        )

    dep_graph = load_dep_graph(gitnexus_dir, str(resolved))
    warnings = gitnexus_warnings(
        params.gitnexus_dir,
        project_root,
        gitnexus_dir,
        dep_graph_loaded=dep_graph is not None,
    )
    assessment = _assess_core(
        baseline_src=baseline_src,
        proposed_src=current_src,
        language=detect_language(resolved),
        priority=priority,
        priority_source=priority_source,
        prefs=prefs,
        dep_graph=dep_graph,
        coupling_for_proposed=dep_graph is not None,
        file_path=resolved,
        allow=params.allow,
        include_security_findings=params.include_security_findings,
        warnings=warnings,
    )

    relocated = _is_complexity_relocated(assessment.metric_deltas)

    entry = ChangesetFileEntry(
        filepath=filepath,
        status=assessment.status,
        is_new=is_new,
        baseline_verdict=None if is_new else assessment.current.lattice_element,
        current_verdict=assessment.proposed.lattice_element,
        score_deltas=assessment.score_deltas,
        metric_deltas=assessment.metric_deltas,
        complexity_relocated_within_file=relocated,
        warnings=assessment.warnings,
    )
    return _FileOutcome(
        entry,
        None if is_new else assessment.current,
        assessment.proposed,
        dep_graph is not None,
        False,
    )


def _is_complexity_relocated(metric_deltas: dict[str, float]) -> bool:
    """Max function complexity improved, but file cyclomatic complexity worsened."""
    func_delta = metric_deltas.get("ast.max_function_complexity", 0.0)
    file_delta = metric_deltas.get("cfg.cyclomatic", 0.0)
    return func_delta < 0.0 and file_delta > 0.0


def _file_error(
    filepath: str, message: str, *, code: str = "path_error"
) -> _FileOutcome:
    from ...schemas import AssessmentStatus

    entry = ChangesetFileEntry(
        filepath=filepath,
        status=AssessmentStatus.LATERAL_MOVE,
        blocked_by=code,
        error=message,
    )
    return _FileOutcome(entry, None, None, False, False)


def _rollup(
    evals: list[EvaluationResult],
) -> tuple[dict[str, LatticeElement], dict[str, float], dict[str, bool]]:
    """Floor each generator across files using the per-file verdict.

    A generator is satisfied project-wide only when every file satisfies it
    (and every file measured it), mirroring ``combine_dimensions``' min-floor.
    """
    n = len(evals)
    ok_count: dict[str, int] = {}
    present_count: dict[str, int] = {}
    min_scores: dict[str, float] = {}

    for e in evals:
        for dim, verdict in e.dimensions.items():
            if dim not in _DIM_VALUE:
                continue
            present_count[dim] = present_count.get(dim, 0) + 1
            passed = e.is_parseable and verdict != LatticeElement.SLOP
            ok_count[dim] = ok_count.get(dim, 0) + (1 if passed else 0)
        for dim, score in e.scores.items():
            if dim not in min_scores or score < min_scores[dim]:
                min_scores[dim] = score

    dims: dict[str, LatticeElement] = {}
    achieved: dict[str, bool] = {}
    for dim in present_count:
        ok = present_count[dim] == n and ok_count.get(dim, 0) == n
        achieved[dim] = ok
        dims[dim] = lattice_to_str(_DIM_VALUE[dim]) if ok else LatticeElement.SLOP
    scores = {dim: round(s, 1) for dim, s in min_scores.items()}
    return dims, scores, achieved


def _aggregate(achieved: dict[str, bool]) -> LatticeElement:
    return lattice_to_str(
        verdict_from_generators(
            simple=achieved.get("simple", False),
            composable=achieved.get("composable", False),
            secure=achieved.get("secure", False),
        )
    )


def _build_changeset_result(
    *,
    params: AssessChangesetInput,
    priority: Priority,
    priority_source: PrioritySource,
    entries: list[ChangesetFileEntry],
    before_evals: list[EvaluationResult],
    after_evals: list[EvaluationResult],
    coupling_available: bool,
) -> ToolResult:
    before_dims, before_scores, before_ok = _rollup(before_evals)
    after_dims, after_scores, after_ok = _rollup(after_evals)

    project_regression = any(
        before_ok.get(dim) and not after_ok.get(dim, False) for dim in before_ok
    )
    relocated_files = [
        e.filepath for e in entries if e.complexity_relocated_within_file
    ]

    model = ChangesetResult(
        baseline_ref=params.baseline_ref,
        files=entries,
        project_before=before_dims,
        project_after=after_dims,
        project_scores_before=before_scores,
        project_scores_after=after_scores,
        aggregate_before=_aggregate(before_ok),
        aggregate_after=_aggregate(after_ok),
        project_regression=project_regression,
        complexity_relocated_files=relocated_files,
        coupling_available=coupling_available,
        priority=priority,
        priority_source=priority_source,
        agent_contract=_changeset_contract(
            project_regression, relocated_files, coupling_available
        ),
    )
    return to_tool_result(model, _render_changeset_md(model))


def _changeset_contract(
    project_regression: bool,
    relocated_files: list[str],
    coupling_available: bool,
) -> AgentContract:
    blocked_by: list[str] = []
    risk_flags: list[str] = []
    next_actions: list[str] = []

    if not coupling_available:
        blocked_by.append("missing_gitnexus_dir")
        risk_flags.append("composable_unavailable")
    if relocated_files:
        risk_flags.append("complexity_relocated_within_file")
        next_actions.append(
            "move extracted logic across a module boundary instead of within one file"
        )
    if project_regression:
        blocked_by.append("project_regression")
        risk_flags.append("project_regression")
        next_tool = "topos_inspect_code"
        next_actions.append("revise the split; the project rollup regressed")
    else:
        next_tool = "topos_evaluate_project"
        next_actions.append("run project rollup and behavior checks before accepting")

    return AgentContract(
        next_tool=next_tool,
        next_actions=next_actions,
        blocked_by=blocked_by,
        verification_gates=[
            "no project_regression in the rollup",
            "no complexity_relocated_within_file flags remain",
            "behavior tests or type/lint checks pass when available",
        ],
        risk_flags=risk_flags,
    )


def _changeset_error(
    priority: Priority,
    priority_source: PrioritySource,
    baseline_ref: str,
    message: str,
) -> ToolResult:
    model = ChangesetResult(
        baseline_ref=baseline_ref,
        priority=priority,
        priority_source=priority_source,
        agent_contract=AgentContract(
            blocked_by=["changeset_error"], risk_flags=["changeset_error"]
        ),
        error=message,
    )
    return to_tool_result(model, _render_changeset_md(model))


def _render_changeset_md(r: ChangesetResult) -> str:
    if r.error:
        return f"**Error:** {r.error}"
    lines = [
        f"**Changeset vs `{r.baseline_ref}`** — "
        f"{r.aggregate_before.value} → {r.aggregate_after.value}",
    ]
    if r.project_regression:
        lines.append("> Project rollup REGRESSED.")
    lines.append("")
    lines.append("## Files")
    lines.append("| File | Status | Before | After | Relocated |")
    lines.append("| --- | --- | --- | --- | --- |")
    for e in r.files:
        before = e.baseline_verdict.value if e.baseline_verdict else "—"
        after = e.current_verdict.value if e.current_verdict else "—"
        reloc = "yes" if e.complexity_relocated_within_file else ""
        safe = e.filepath.replace("|", "\\|")
        lines.append(f"| `{safe}` | {e.status.value} | {before} | {after} | {reloc} |")
    if r.complexity_relocated_files:
        lines.append("")
        lines.append(
            "**Complexity relocated within file:** "
            + ", ".join(f"`{f}`" for f in r.complexity_relocated_files)
        )
    # The rollup floors each generator across files, so a brand-new file that
    # is not yet clean can drag the whole project down. Call that out so the
    # regression isn't misread as a fault in the edited files.
    new_slop = [
        e.filepath
        for e in r.files
        if e.is_new and e.current_verdict == LatticeElement.SLOP
    ]
    if r.project_regression and new_slop:
        lines.append("")
        lines.append(
            "> Rollup floor dragged down by new, not-yet-clean file(s): "
            + ", ".join(f"`{f}`" for f in new_slop)
            + " — regression may reflect unfinished new modules, not the edits."
        )
    return "\n".join(lines)
