"""
Evaluation tool: whole project.
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import replace
from pathlib import Path

from fastmcp import Context
from fastmcp.tools.base import ToolResult

from topos.core.omega import EvaluationValue
from topos.evaluation.characteristic_morphism import (
    CharacteristicMorphism,
    ClassificationResult,
)
from topos.evaluation.policies.base import Priority
from topos.graphs.ast.dispatch import LANGUAGE_FILE_SUFFIXES
from topos.utils.discovery import collect_source_files

from ...diagnostics import overlay_for_file
from ...evaluation import (
    classify_file,
    detect_language,
    gitnexus_warnings,
    resolve_gitnexus_dir,
)
from ...formatting import (
    build_pillars,
    composable_contract_signals,
    lattice_to_str,
    to_tool_result,
)
from ...schemas import (
    AgentContract,
    EvaluateProjectInput,
    LatticeElement,
    PrioritySource,
    ProjectEvaluationResult,
    ProjectFileEntry,
    ProjectLanguageRollup,
    resolve_priority,
)
from ...security import resolve_file_root, resolve_within_root
from ...server import mcp
from .render import render_project_md

_READ_ONLY_ANN = {
    "title": "Topos Code Evaluation",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}

_SUPPORTED_SOURCE_SUFFIXES = tuple(
    sorted(
        {suffix for suffixes in LANGUAGE_FILE_SUFFIXES.values() for suffix in suffixes}
    )
)


def _adjusted_result(result: ClassificationResult, overlay):
    if overlay is None:
        return result
    dimensions = dict(result.dimensions)
    scores = dict(result.scores)
    dimensions["secure"] = (
        EvaluationValue.SECURE
        if overlay.verdict.adjusted_secure_pass
        else EvaluationValue.SLOP
    )
    scores["secure"] = 1.0 if overlay.verdict.adjusted_secure_pass else 0.0
    return replace(
        result,
        dimensions=dimensions,
        scores=scores,
        lattice_element=overlay.verdict.adjusted_element,
    )


def _evaluate_single_file(
    path: Path,
    resolved_root: Path,
    priority: Priority,
    gitnexus_dir: Path | None,
    include_security_findings: bool,
    allows: list[str],
) -> tuple[ClassificationResult | None, ProjectFileEntry | None, bool, bool]:
    try:
        result, dep_graph = classify_file(path, priority, gitnexus_dir)
    except Exception:
        return None, None, True, False

    is_parse_failure = not result.is_parseable
    warnings: list[str] = []
    overlay = overlay_for_file(
        path,
        result,
        allows=allows,
        include_security_findings=include_security_findings,
    )
    adjusted = overlay.verdict if overlay else None
    result_for_rollup = _adjusted_result(result, overlay)

    entry = ProjectFileEntry(
        filepath=str(path.relative_to(resolved_root)),
        language=detect_language(path),
        lattice_element=lattice_to_str(result_for_rollup.summary()),
        scores={dim: round(s * 100.0, 1) for dim, s in result.scores.items()},
        pillars=build_pillars(result_for_rollup, dep_graph is not None),
        raw_metrics=dict(result.raw_metrics),
        warnings=warnings,
        security_findings=overlay.active_findings if overlay else [],
        acknowledged_risks=overlay.acknowledged_risks if overlay else [],
        raw_lattice_element=(
            lattice_to_str(adjusted.raw_element) if adjusted else None
        ),
        adjusted_lattice_element=(
            lattice_to_str(adjusted.adjusted_element) if adjusted else None
        ),
        secure_raw=adjusted.raw_secure_pass if adjusted else None,
        secure_adjusted=adjusted.adjusted_secure_pass if adjusted else None,
        grade_capped=adjusted.grade_capped if adjusted else False,
        is_parseable=result.is_parseable,
    )
    return result_for_rollup, entry, is_parse_failure, dep_graph is not None


def _validate_and_collect_project(
    params: EvaluateProjectInput,
) -> tuple[Path | None, list[Path] | None, str | None]:
    resolved_root, err = resolve_within_root(params.path)
    if err or resolved_root is None:
        return None, None, (err or {}).get("error") or "Access denied"

    if not resolved_root.is_dir():
        return None, None, f"Path is not a directory: {resolved_root}"

    source_files = collect_source_files(
        (str(resolved_root),),
        suffixes=_SUPPORTED_SOURCE_SUFFIXES,
        recursive=True,
    )
    if not source_files:
        return None, None, "No supported source files found."

    return resolved_root, source_files, None


async def _evaluate_project_files_loop(
    source_files: list[Path],
    resolved_root: Path,
    priority: Priority,
    gitnexus_dir: Path | None,
    include_security_findings: bool,
    allows: list[str],
    ctx: Context,
) -> tuple[
    list[ClassificationResult],
    list[ProjectFileEntry],
    int,
    bool,
    dict[str, list[ClassificationResult]],
    dict[str, list[ProjectFileEntry]],
    dict[str, int],
]:
    total_files = len(source_files)
    per_file_results = []
    entries: list[ProjectFileEntry] = []
    parse_failures = 0
    any_dep_graph_loaded = False
    per_language_results: dict[str, list[ClassificationResult]] = defaultdict(list)
    per_language_entries: dict[str, list[ProjectFileEntry]] = defaultdict(list)
    per_language_parse_failures: dict[str, int] = defaultdict(int)

    for idx, path in enumerate(source_files, start=1):
        language = detect_language(path)
        result, entry, failed, has_dep = _evaluate_single_file(
            path,
            resolved_root,
            priority,
            gitnexus_dir,
            include_security_findings,
            allows,
        )
        if failed:
            parse_failures += 1
            per_language_parse_failures[language] += 1
        if result is None or entry is None:
            continue
        any_dep_graph_loaded = any_dep_graph_loaded or has_dep
        # NOTE: ``failed`` above already reflects ``not result.is_parseable`` on
        # the success path (see ``_evaluate_single_file``), so parse failures are
        # counted there once — re-counting here would double the tally.
        per_file_results.append(result)
        entries.append(entry)
        per_language_results[language].append(result)
        per_language_entries[language].append(entry)
        if idx % max(1, total_files // 20) == 0 or idx == total_files:
            await ctx.report_progress(progress=idx, total=total_files)

    return (
        per_file_results,
        entries,
        parse_failures,
        any_dep_graph_loaded,
        dict(per_language_results),
        dict(per_language_entries),
        dict(per_language_parse_failures),
    )


@mcp.tool(
    name="topos_evaluate_project",
    tags={"evaluate", "project"},
    annotations=_READ_ONLY_ANN,
)
async def topos_evaluate_project(
    params: EvaluateProjectInput, ctx: Context
) -> ToolResult:
    """Recursively score every supported source file in a directory on the
    SIMPLE / COMPOSABLE / SECURE lattice, with a project rollup (read-only).

    Use for a whole tree; for a single file use ``topos_evaluate_file``, for a
    snippet ``topos_evaluate_code``. Autodetects all supported languages
    (Python, Rust, JavaScript, TypeScript, C++) in one walk — no language
    argument — and skips unsupported files. Import/export-only entrypoint
    modules (``__init__.py``, ``index.ts``, ``mod.rs``) are exempted so trivial
    re-export hubs don't floor the verdict. The rollup takes the project-wide
    minimum per dimension (weakest file floors it); progress streams via
    ``ctx.report_progress``. Returns a paginated per-file table (worst first)
    plus per-language rollups; page with ``limit`` / ``offset``.
    """
    resolved_root, source_files, err_msg = _validate_and_collect_project(params)
    if err_msg or resolved_root is None or source_files is None:
        model = _empty_project_result(params, error=err_msg)
        return to_tool_result(model, render_project_md(model))

    project_root = resolve_file_root()
    gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir, project_root)
    priority, priority_source = resolve_priority(params.preferences)
    coupling_available = gitnexus_dir is not None

    (
        per_file_results,
        entries,
        parse_failures,
        any_dep_graph_loaded,
        per_language_results,
        per_language_entries,
        per_language_parse_failures,
    ) = await _evaluate_project_files_loop(
        source_files,
        resolved_root,
        priority,
        gitnexus_dir,
        params.include_security_findings,
        params.allow,
        ctx,
    )

    model = _build_project_result(
        resolved_root,
        source_files,
        parse_failures,
        per_file_results,
        entries,
        any_dep_graph_loaded,
        per_language_results,
        per_language_entries,
        per_language_parse_failures,
        params,
        priority,
        priority_source,
        coupling_available,
        project_root,
        gitnexus_dir,
    )
    return to_tool_result(model, render_project_md(model))


def _build_project_result(
    resolved_root: Path,
    source_files: list[Path],
    parse_failures: int,
    per_file_results: list[ClassificationResult],
    entries: list[ProjectFileEntry],
    any_dep_graph_loaded: bool,
    per_language_results: dict[str, list[ClassificationResult]],
    per_language_entries: dict[str, list[ProjectFileEntry]],
    per_language_parse_failures: dict[str, int],
    params: EvaluateProjectInput,
    priority: Priority,
    priority_source: PrioritySource,
    coupling_available: bool,
    project_root: Path,
    gitnexus_dir: Path | None,
) -> ProjectEvaluationResult:
    classifier = CharacteristicMorphism()
    rolled = classifier.combine_dimensions(per_file_results)
    rolled_scores = _minimum_scores_by_dim(per_file_results)
    language_rollups = _build_language_rollups(
        per_language_results,
        per_language_entries,
        per_language_parse_failures,
    )

    # Combine the three rolled-up generator verdicts into a single ℋ
    # element via the free-algebra encoding.
    overall = _aggregate_floor_verdict(rolled)
    aggregate_explanation = _aggregate_explanation(rolled, rolled_scores, entries)

    # Sort entries: lowest overall score first (worst files surfaced).
    entries.sort(key=lambda e: min(e.scores.values()) if e.scores else 0.0)
    worst_files = entries[: min(3, len(entries))]
    worst_file_verdict = worst_files[0].lattice_element if worst_files else None
    guidance = _project_guidance(worst_files)

    page = entries[params.offset : params.offset + params.limit]
    has_more = params.offset + len(page) < len(entries)
    next_offset = params.offset + len(page) if has_more else None

    project_warnings = gitnexus_warnings(
        params.gitnexus_dir,
        project_root,
        gitnexus_dir,
        dep_graph_loaded=any_dep_graph_loaded,
    )
    next_tool, next_actions, blocked_by, verification_gates, risk_flags = (
        _project_contract(
            overall,
            worst_files,
            coupling_available,
            project_warnings,
            parse_failures,
        )
    )

    return ProjectEvaluationResult(
        root=str(resolved_root),
        file_count=len(source_files),
        parse_failures=parse_failures,
        rolled_up_dimensions={dim: lattice_to_str(val) for dim, val in rolled.items()},
        rolled_up_scores=rolled_scores,
        aggregate_floor_verdict=overall,
        language_rollups=language_rollups,
        aggregate_explanation=aggregate_explanation,
        worst_file_verdict=worst_file_verdict,
        worst_files=worst_files,
        guidance=guidance,
        priority=priority,
        priority_source=priority_source,
        coupling_available=coupling_available,
        warnings=project_warnings,
        agent_contract=AgentContract(
            next_tool=next_tool,
            next_actions=next_actions,
            blocked_by=blocked_by,
            verification_gates=verification_gates,
            risk_flags=risk_flags,
        ),
        count=len(page),
        offset=params.offset,
        total=len(entries),
        has_more=has_more,
        next_offset=next_offset,
        files=page,
        verbose=params.verbose,
    )


def _minimum_scores_by_dim(results) -> dict[str, float]:
    min_scores: dict[str, float] = {}
    for r in results:
        for dim, s in r.scores.items():
            if dim not in min_scores or s < min_scores[dim]:
                min_scores[dim] = s
    return {dim: round(s * 100.0, 1) for dim, s in min_scores.items()}


def _aggregate_floor_verdict(rolled: dict[str, object]) -> LatticeElement:
    from topos.core.omega import verdict_from_generators

    simple_ok = rolled.get("simple") == EvaluationValue.SIMPLE
    composable_ok = rolled.get("composable") == EvaluationValue.COMPOSABLE
    secure_ok = rolled.get("secure") == EvaluationValue.SECURE
    return lattice_to_str(
        verdict_from_generators(
            simple=simple_ok, composable=composable_ok, secure=secure_ok
        )
    )


def _build_language_rollups(
    per_language_results: dict[str, list[ClassificationResult]],
    per_language_entries: dict[str, list[ProjectFileEntry]],
    per_language_parse_failures: dict[str, int],
) -> list[ProjectLanguageRollup]:
    classifier = CharacteristicMorphism()
    rollups: list[ProjectLanguageRollup] = []
    for language in sorted(per_language_results):
        results = per_language_results[language]
        rolled = classifier.combine_dimensions(results)
        rolled_scores = _minimum_scores_by_dim(results)
        entries = sorted(
            per_language_entries.get(language, []),
            key=lambda e: min(e.scores.values()) if e.scores else 0.0,
        )
        worst = entries[0] if entries else None
        rollups.append(
            ProjectLanguageRollup(
                language=language,
                file_count=len(entries),
                parse_failures=per_language_parse_failures.get(language, 0),
                rolled_up_dimensions={
                    dim: lattice_to_str(val) for dim, val in rolled.items()
                },
                rolled_up_scores=rolled_scores,
                aggregate_floor_verdict=_aggregate_floor_verdict(rolled),
                worst_file_path=worst.filepath if worst else None,
                worst_file_verdict=worst.lattice_element if worst else None,
            )
        )
    return rollups


def _empty_project_result(
    params: EvaluateProjectInput, error: str | None
) -> ProjectEvaluationResult:
    priority, priority_source = resolve_priority(params.preferences)
    return ProjectEvaluationResult(
        root=params.path,
        file_count=0,
        parse_failures=0,
        rolled_up_dimensions={},
        rolled_up_scores={},
        aggregate_floor_verdict=LatticeElement.SLOP,
        aggregate_explanation=(
            "No files were evaluated, so the aggregate floor is SLOP."
        ),
        worst_file_verdict=None,
        worst_files=[],
        guidance=error or "No project guidance available.",
        priority=priority,
        priority_source=priority_source,
        coupling_available=False,
        agent_contract=AgentContract(
            blocked_by=["project_evaluation_error"] if error else [],
            risk_flags=["project_evaluation_error"] if error else [],
        ),
        count=0,
        offset=params.offset,
        total=0,
        has_more=False,
        next_offset=None,
        files=[],
        verbose=params.verbose,
        error=error,
    )


def _aggregate_explanation(
    rolled: dict[str, object],
    rolled_scores: dict[str, float],
    entries: list[ProjectFileEntry],
) -> str:
    if not entries:
        return "No files were evaluated, so the aggregate floor is SLOP."
    failed = [
        dim for dim, val in rolled.items() if lattice_to_str(val) == LatticeElement.SLOP
    ]
    worst = min(entries, key=lambda e: min(e.scores.values()) if e.scores else 0.0)
    if failed:
        dim = min(failed, key=lambda name: rolled_scores.get(name, 100.0))
        score = rolled_scores.get(dim)
        score_text = f" ({score:.1f}%)" if score is not None else ""
        return (
            "Aggregate floor is SLOP because at least one file fails "
            f"{dim}{score_text}; "
            f"worst current target is {worst.filepath} ({worst.lattice_element.value})."
        )
    return (
        "Aggregate floor satisfies every measured generator; worst current target is "
        f"{worst.filepath} ({worst.lattice_element.value})."
    )


def _project_guidance(worst_files: list[ProjectFileEntry]) -> str:
    if not worst_files:
        return "No files were evaluated."
    worst = worst_files[0]
    if worst.warnings:
        return f"Start with `{worst.filepath}`: {worst.warnings[0]}"
    if worst.scores:
        dim = min(worst.scores, key=worst.scores.get)
        return f"Start with `{worst.filepath}`; weakest measured generator is {dim}."
    return f"Start with `{worst.filepath}`; inspect parseability and raw metrics."


def _project_contract(
    overall: LatticeElement,
    worst_files: list[ProjectFileEntry],
    coupling_available: bool,
    warnings: list[str],
    parse_failures: int,
) -> tuple[str | None, list[str], list[str], list[str], list[str]]:
    blocked_by: list[str] = []
    risk_flags: list[str] = []
    next_actions: list[str] = []

    composable = composable_contract_signals(
        coupling_available=coupling_available,
        warnings=warnings,
    )
    blocked_by.extend(composable.blocked_by)
    risk_flags.extend(composable.risk_flags)
    if parse_failures:
        blocked_by.append("parse_failures")
        risk_flags.append("parse_failures")
    if warnings:
        risk_flags.append("warnings")
    if any(f.grade_capped for f in worst_files):
        risk_flags.append("grade_capped")
    if any(f.security_findings for f in worst_files):
        risk_flags.append("active_security_findings")

    verification_gates = [
        "topos_assess_worktree_change validates each accepted in-place refactor",
        "project rollup does not regress after non-trivial changes",
        "behavior tests or type/lint checks pass when available",
    ]
    if composable.next_action:
        return (
            composable.next_tool,
            [composable.next_action],
            blocked_by,
            verification_gates,
            risk_flags,
        )
    if not worst_files:
        return None, [], blocked_by, [], risk_flags
    if overall == LatticeElement.IDEAL:
        next_tool = None
        next_actions.append("preserve behavior checks before accepting")
    else:
        next_tool = "topos_inspect_code"
        worst = worst_files[0]
        next_actions.append(
            f"start with worst file {worst.filepath} using language {worst.language}"
        )

    return next_tool, next_actions, blocked_by, verification_gates, risk_flags
