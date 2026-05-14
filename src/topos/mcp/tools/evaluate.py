"""
Evaluation tools: code string, single file, whole project.

The single-file tool is the P0 bug fix — the previous ``evaluate_file``
delegated to ``evaluate_code`` and dropped the filepath, so ModuleDependencyGraph
was never built and COMPOSABLE/SOUND were unreachable via MCP.
"""

from __future__ import annotations

from fastmcp import Context

from topos.evaluation.characteristic_morphism import CharacteristicMorphism

from ..evaluation import (
    classify_code_string,
    classify_file,
    resolve_gitnexus_dir,
)
from ..formatting import (
    lattice_to_str,
    to_evaluation_result,
)
from ..schemas import (
    EvaluateCodeInput,
    EvaluateFileInput,
    EvaluateProjectInput,
    EvaluationResult,
    LatticeElement,
    ProjectEvaluationResult,
    ProjectFileEntry,
)
from ..security import resolve_file_root, resolve_within_root
from ..server import mcp

_READ_ONLY_ANN = {
    "title": "Topos Code Evaluation",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}


@mcp.tool(
    name="topos_evaluate_code",
    tags={"evaluate", "single-unit"},
    annotations=_READ_ONLY_ANN,
)
def topos_evaluate_code(params: EvaluateCodeInput) -> EvaluationResult:
    """Classify a raw code string on the free Heyting algebra H(G_qual).

    SIMPLE and SECURE generators are scored from CFG / CPG built on the
    source.  COMPOSABLE requires a ``ModuleDependencyGraph`` (and is therefore
    unreachable from a bare string); use ``topos_evaluate_file`` with
    ``gitnexus_dir`` to enable it.

    Lattice values are the 8 elements of the 3-cube H(G_qual):
        SLOP (⊥)             No generator satisfied.
        SIMPLE / COMPOSABLE / SECURE     One generator satisfied.
        SIMPLE_COMPOSABLE / SIMPLE_SECURE / COMPOSABLE_SECURE  Two satisfied.
        IDEAL (⊤)            All three generators satisfied.
    """
    try:
        result = classify_code_string(params.code, params.language, params.priority)
    except Exception as exc:
        return EvaluationResult(
            is_parseable=False,
            lattice_element=LatticeElement.SLOP,
            lattice_symbol="⊥",
            lattice_description="Evaluation failed",
            dimensions={},
            scores={},
            priority=params.priority,
            guidance="",
            coupling_available=False,
            error=str(exc),
        )
    prefs = params.preferences.to_preferences() if params.preferences else None
    return to_evaluation_result(result, coupling_available=False, preferences=prefs)


@mcp.tool(
    name="topos_evaluate_file",
    tags={"evaluate", "single-unit"},
    annotations=_READ_ONLY_ANN,
)
def topos_evaluate_file(params: EvaluateFileInput) -> EvaluationResult:
    """Evaluate a file on disk. **Enables the COMPOSABLE generator.**

    When ``gitnexus_dir`` is provided (or auto-detected at
    ``<project_root>/.gitnexus``), a ``ModuleDependencyGraph`` is attached to
    the classifier so the COMPOSABLE generator can be scored.  CFG and
    CPG (SIMPLE and SECURE generators) always run.

    Generate a ``.gitnexus/`` directory with ``topos depgraph generate`` first
    (requires ``npm install -g gitnexus``).
    """
    resolved, err = resolve_within_root(params.filepath)
    if err or resolved is None:
        return EvaluationResult(
            is_parseable=False,
            lattice_element=LatticeElement.SLOP,
            lattice_symbol="⊥",
            lattice_description="Access denied / path error",
            dimensions={},
            scores={},
            priority=params.priority,
            guidance="",
            coupling_available=False,
            error=(err or {}).get("error", "path error"),
        )

    if not resolved.is_file():
        return EvaluationResult(
            is_parseable=False,
            lattice_element=LatticeElement.SLOP,
            lattice_symbol="⊥",
            lattice_description="Not a file",
            dimensions={},
            scores={},
            priority=params.priority,
            guidance="",
            coupling_available=False,
            error=f"Path is not a file: {resolved}",
        )

    project_root = resolve_file_root()
    gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir, project_root)

    try:
        result, dep_graph = classify_file(resolved, params.priority, gitnexus_dir)
    except Exception as exc:
        return EvaluationResult(
            is_parseable=False,
            lattice_element=LatticeElement.SLOP,
            lattice_symbol="⊥",
            lattice_description="Evaluation failed",
            dimensions={},
            scores={},
            priority=params.priority,
            guidance="",
            coupling_available=False,
            error=str(exc),
        )

    prefs = params.preferences.to_preferences() if params.preferences else None
    return to_evaluation_result(
        result, coupling_available=dep_graph is not None, preferences=prefs
    )


@mcp.tool(
    name="topos_evaluate_project",
    tags={"evaluate", "project"},
    annotations=_READ_ONLY_ANN,
)
async def topos_evaluate_project(
    params: EvaluateProjectInput, ctx: Context
) -> ProjectEvaluationResult:
    """Recursively evaluate every Python file in a directory.

    Reports progress to the client via ``ctx.report_progress`` so the UI shows
    a live bar during long walks. Rolls up per-dimension scores using the
    project-wide minimum (``CharacteristicMorphism.combine_dimensions``).

    Returns a paginated per-file table plus the overall rollup. Use ``limit``
    / ``offset`` to page through large codebases.
    """
    resolved_root, err = resolve_within_root(params.path)
    if err or resolved_root is None:
        return _empty_project_result(params, error=(err or {}).get("error"))

    if not resolved_root.is_dir():
        return _empty_project_result(
            params, error=f"Path is not a directory: {resolved_root}"
        )

    py_files = sorted(resolved_root.rglob("*.py"))
    total_files = len(py_files)
    if total_files == 0:
        return _empty_project_result(params, error="No .py files found.")

    gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir, resolve_file_root())
    coupling_available = gitnexus_dir is not None

    per_file_results = []
    entries: list[ProjectFileEntry] = []
    parse_failures = 0

    for idx, path in enumerate(py_files, start=1):
        try:
            result, _ = classify_file(path, params.priority, gitnexus_dir)
        except Exception:
            parse_failures += 1
            continue
        if not result.is_parseable:
            parse_failures += 1
        per_file_results.append(result)
        entries.append(
            ProjectFileEntry(
                filepath=str(path.relative_to(resolved_root)),
                lattice_element=lattice_to_str(result.summary()),
                scores={dim: round(s * 100.0, 1) for dim, s in result.scores.items()},
                raw_metrics=dict(result.raw_metrics),
                is_parseable=result.is_parseable,
            )
        )
        if idx % max(1, total_files // 20) == 0 or idx == total_files:
            await ctx.report_progress(progress=idx, total=total_files)

    classifier = CharacteristicMorphism()
    rolled = classifier.combine_dimensions(per_file_results)
    rolled_scores = _minimum_scores_by_dim(per_file_results)

    # Combine the three rolled-up generator verdicts into a single ℋ
    # element via the free-algebra encoding.
    from topos.core.omega import (
        EvaluationValue,
        verdict_from_generators,
    )

    simple_ok = rolled.get("simple") == EvaluationValue.SIMPLE
    composable_ok = rolled.get("composable") == EvaluationValue.COMPOSABLE
    secure_ok = rolled.get("secure") == EvaluationValue.SECURE
    overall_value = verdict_from_generators(
        simple=simple_ok, composable=composable_ok, secure=secure_ok
    )
    overall = lattice_to_str(overall_value)

    # Sort entries: lowest overall score first (worst files surfaced).
    entries.sort(key=lambda e: min(e.scores.values()) if e.scores else 0.0)

    page = entries[params.offset : params.offset + params.limit]
    has_more = params.offset + len(page) < len(entries)
    next_offset = params.offset + len(page) if has_more else None

    return ProjectEvaluationResult(
        root=str(resolved_root),
        file_count=total_files,
        parse_failures=parse_failures,
        rolled_up_dimensions={dim: lattice_to_str(val) for dim, val in rolled.items()},
        rolled_up_scores=rolled_scores,
        overall=overall,
        priority=params.priority,
        coupling_available=coupling_available,
        count=len(page),
        offset=params.offset,
        total=len(entries),
        has_more=has_more,
        next_offset=next_offset,
        files=page,
    )


def _minimum_scores_by_dim(results) -> dict[str, float]:
    min_scores: dict[str, float] = {}
    for r in results:
        for dim, s in r.scores.items():
            if dim not in min_scores or s < min_scores[dim]:
                min_scores[dim] = s
    return {dim: round(s * 100.0, 1) for dim, s in min_scores.items()}


def _empty_project_result(
    params: EvaluateProjectInput, error: str | None
) -> ProjectEvaluationResult:
    return ProjectEvaluationResult(
        root=params.path,
        file_count=0,
        parse_failures=0,
        rolled_up_dimensions={},
        rolled_up_scores={},
        overall=LatticeElement.SLOP,
        priority=params.priority,
        coupling_available=False,
        count=0,
        offset=params.offset,
        total=0,
        has_more=False,
        next_offset=None,
        files=[],
        error=error,
    )


# ---------------------------------------------------------------------------
# Markdown helpers (used by assess.py when response_format=markdown)
# ---------------------------------------------------------------------------


def render_project_md(r: ProjectEvaluationResult) -> str:
    lines = [f"# Project Evaluation — {r.root}", ""]
    lines.append(f"**Overall:** {r.overall.value}")
    lines.append(
        f"**Files scanned:** {r.file_count} (parse failures: {r.parse_failures})"
    )
    lines.append(f"**Priority:** `{r.priority.value}`")
    if not r.coupling_available:
        lines.append("> ⚠️ No `.gitnexus/` present — coupling dimension not scored.")
    lines.append("")
    lines.append("## Rolled-up dimensions")
    for dim, val in r.rolled_up_dimensions.items():
        s = r.rolled_up_scores.get(dim)
        lines.append(
            f"- **{dim}**: {val.value}" + (f" ({s:.1f}%)" if s is not None else "")
        )
    lines.append("")
    lines.append(f"## Worst files (showing {r.count} of {r.total}, offset {r.offset})")
    for entry in r.files:
        s_str = ", ".join(f"{k}={v:.0f}" for k, v in entry.scores.items())
        lines.append(f"- `{entry.filepath}` — {entry.lattice_element.value} ({s_str})")
    if r.has_more:
        lines.append(
            f"\n_more files available: pass offset={r.next_offset} to continue._"
        )
    if r.error:
        lines.append(f"\n> error: {r.error}")
    return "\n".join(lines)
