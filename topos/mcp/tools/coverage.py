"""
Coverage tools — structural test coverage (UAST) and topological ECT coverage.
"""

from __future__ import annotations

from fastmcp.tools.base import ToolResult

from topos.core.morphism import ProgramMorphism
from topos.evaluation.policies.coverage import (
    score_topological_coverage,
)
from topos.functors.profunctors.cpg.topological_coverage import (
    ECT_COVERAGE_INSTALL_HINT,
    ECTCoverageUnavailableError,
    calculate_topological_coverage,
    ect_coverage_available,
)
from topos.functors.profunctors.uast.structural_test_coverage import (
    declaration_coverage,
)
from topos.graphs.cpg.object import CodePropertyGraph

from ..formatting import to_tool_result
from ..schemas import (
    CalculateCoverageInput,
    CoverageResult,
    TopologicalCoverageResult,
)
from ..security import resolve_within_root
from ..server import mcp

_READ_ONLY_ANN = {
    "title": "Topos Structural Coverage",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}


def _empty_coverage_result(
    warnings: list[str],
    error: str,
) -> CoverageResult:
    return CoverageResult(
        mean_declaration_coverage=0.0,
        best_declaration_recall=[],
        declaration_locations=[],
        stmt_recall=0.0,
        expr_recall=0.0,
        mean_test_precision=0.0,
        f2_score=0.0,
        declaration_path_recall_kgram=0.0,
        uncovered_declarations=[],
        put_declaration_count=0,
        test_declaration_count=0,
        warnings=warnings,
        error=error,
    )


def _merge_cpgs(cpgs: list[CodePropertyGraph]) -> CodePropertyGraph:
    if not cpgs:
        return CodePropertyGraph()
    merged_nodes = {}
    merged_edges = []
    sources: list[str] = []
    for cpg in cpgs:
        merged_nodes.update(cpg.nodes)
        merged_edges.extend(cpg.edges)
        if cpg.source:
            sources.append(cpg.source)
    return CodePropertyGraph(
        nodes=merged_nodes,
        edges=merged_edges,
        language=cpgs[0].language,
        source="\n".join(sources),
    )


def _build_file_cpgs(files: list[str], language: str) -> list[CodePropertyGraph]:
    cpgs: list[CodePropertyGraph] = []
    for path in files:
        resolved, err = resolve_within_root(path)
        if err or resolved is None:
            continue
        morphism = ProgramMorphism.from_file(resolved, language=language)
        cpg = morphism.build_cpg()
        if cpg is not None:
            cpgs.append(cpg)
    return cpgs


def _compute_topological_coverage(
    put_files: list[str],
    test_files: list[str],
    language: str,
    threshold: float,
) -> TopologicalCoverageResult:
    if not ect_coverage_available():
        return TopologicalCoverageResult(
            unavailable=True,
            reason=(
                "Topological (ECT) coverage requires the optional ect-coverage extra. "
                f"Install with: {ECT_COVERAGE_INSTALL_HINT}"
            ),
        )

    put_cpgs = _build_file_cpgs(put_files, language)
    test_cpgs = _build_file_cpgs(test_files, language)

    try:
        topo_report = calculate_topological_coverage(
            _merge_cpgs(put_cpgs),
            _merge_cpgs(test_cpgs),
        )
        topo_decision = score_topological_coverage(topo_report, threshold=threshold)
    except ECTCoverageUnavailableError as exc:
        return TopologicalCoverageResult(unavailable=True, reason=str(exc))

    return TopologicalCoverageResult(
        distance=topo_report.topological_distance,
        coverage_score=topo_report.topological_coverage_score,
        tested_functions=list(topo_report.tested_functions),
        untested_functions=list(topo_report.untested_functions),
        put_node_count=topo_report.put_node_count,
        test_node_count=topo_report.test_node_count,
        scoped_node_count=topo_report.scoped_node_count,
        achieved=topo_decision.achieved,
        threshold=topo_decision.threshold,
        interpretation=dict(topo_decision.interpretation),
    )


def _parse_roots(
    files: list[str], language: str, warnings: list[str], label: str
) -> tuple[list, CoverageResult | None]:
    roots = []
    for path in files:
        resolved, err = resolve_within_root(path)
        if err or resolved is None:
            model = _empty_coverage_result(
                warnings,
                (
                    f"{label} file error: {(err or {}).get('error', 'path error')} "
                    f"for {path}"
                ),
            )
            return [], model
        try:
            morphism = ProgramMorphism.from_file(resolved, language=language)
            if morphism.ast and morphism.ast.uast_root:
                roots.append(morphism.ast.uast_root)
        except Exception as exc:
            model = _empty_coverage_result(
                warnings,
                f"Failed to parse {label} file {path}: {exc}",
            )
            return [], model
    return roots, None


@mcp.tool(
    name="topos_calculate_coverage",
    tags={"coverage", "uast"},
    annotations=_READ_ONLY_ANN,
)
def topos_calculate_coverage(params: CalculateCoverageInput) -> ToolResult:
    """Measure structural test coverage of program-under-test files against tests.

    Read-only; a standalone signal OUTSIDE the quality lattice (never changes a
    medal). Uses UAST kind histograms, statement/expression recall, and k-gram
    path recall; with the ``ect-coverage`` extra it also returns topological ECT
    coverage. Returns a CoverageResult: ``mean_declaration_coverage`` in [0, 1],
    recall metrics, ``f2_score``, ``uncovered_declarations``, and optional
    ``topological_coverage``.
    """
    warnings: list[str] = []
    put_roots, err_model = _parse_roots(
        params.put_files, params.language, warnings, "PUT"
    )
    if err_model is not None:
        return to_tool_result(err_model, render_coverage_md(err_model))

    test_roots, err_model = _parse_roots(
        params.test_files, params.language, warnings, "Test"
    )
    if err_model is not None:
        return to_tool_result(err_model, render_coverage_md(err_model))

    if not put_roots:
        model = _empty_coverage_result(
            warnings,
            "No valid PUT roots found (parsing failed or files empty).",
        )
        return to_tool_result(model, render_coverage_md(model))

    try:
        report = declaration_coverage(
            put_roots=put_roots,
            test_roots=test_roots,
            k=params.k,
            include_unknown=params.include_unknown,
        )
        topological = _compute_topological_coverage(
            params.put_files,
            params.test_files,
            params.language,
            params.coverage_threshold,
        )
        if topological.unavailable:
            warnings = [
                *warnings,
                topological.reason or "Topological coverage unavailable.",
            ]
        model = CoverageResult(
            mean_declaration_coverage=report.mean_declaration_coverage,
            best_declaration_recall=list(report.best_declaration_recall),
            declaration_locations=list(report.declaration_locations),
            stmt_recall=report.stmt_recall,
            expr_recall=report.expr_recall,
            mean_test_precision=report.mean_test_precision,
            f2_score=report.f2_score,
            declaration_path_recall_kgram=report.declaration_path_recall_kgram,
            uncovered_declarations=report.uncovered_declarations,
            put_declaration_count=report.put_declaration_count,
            test_declaration_count=report.test_declaration_count,
            topological_coverage=topological,
            warnings=warnings,
        )
        return to_tool_result(model, render_coverage_md(model))
    except Exception as exc:
        model = _empty_coverage_result(
            warnings,
            f"Coverage calculation failed: {exc}",
        )
        return to_tool_result(model, render_coverage_md(model))


def _render_uncovered(r: CoverageResult) -> list[str]:
    lines = []
    if r.uncovered_declarations:
        lines.append("## Uncovered Declarations")
        for loc in r.uncovered_declarations:
            try:
                idx = r.declaration_locations.index(loc)
                recall = r.best_declaration_recall[idx]
                lines.append(f"- `{loc}` ({recall * 100:.1f}%)")
            except (ValueError, IndexError):
                lines.append(f"- `{loc}`")
    else:
        lines.append("## ✅ 100% Structural Coverage")
        lines.append(
            "All declarations in the PUT are structurally represented in the test "
            "suite."
        )
    return lines


def _render_topological_coverage(topo) -> list[str]:
    lines = ["", "## Topological CPG Semantic Coverage (ECT)"]
    if topo.unavailable:
        lines.append(f"> Topological coverage unavailable: {topo.reason}")
    elif topo.coverage_score is not None:
        lines.append(f"- **Coverage score:** {topo.coverage_score:.4f}")
        if topo.distance is not None:
            lines.append(f"- **ECT distance:** {topo.distance:.4f}")
        if topo.achieved is not None and topo.threshold is not None:
            lines.append(
                f"- **Threshold met:** {topo.achieved} (threshold {topo.threshold:.2f})"
            )
        if topo.scoped_node_count is not None:
            lines.append(f"- **Scoped PUT nodes:** {topo.scoped_node_count}")
        if topo.tested_functions:
            lines.append(f"- **Tested functions:** {', '.join(topo.tested_functions)}")
        if topo.untested_functions:
            lines.append(
                f"- **Untested functions:** {', '.join(topo.untested_functions)}"
            )
    return lines


def render_coverage_md(r: CoverageResult) -> str:
    """Markdown rendering of a structural coverage result."""
    lines = ["# Structural Test Coverage (UAST)", ""]

    if r.error:
        lines.append(f"> ⚠️ **Error:** {r.error}")
        return "\n".join(lines)

    lines.append(
        f"**Mean Declaration Coverage:** {r.mean_declaration_coverage * 100:.1f}%"
    )
    lines.append(f"**F2 Score (Recall-weighted):** {r.f2_score:.3f}")
    lines.append(f"**Test Suite Precision:** {r.mean_test_precision * 100:.1f}%")
    lines.append("")

    lines.append("## Stratified Recall")
    lines.append(f"- **Statements:** {r.stmt_recall * 100:.1f}%")
    lines.append(f"- **Expressions:** {r.expr_recall * 100:.1f}%")
    lines.append(f"- **Paths (k-gram):** {r.declaration_path_recall_kgram * 100:.1f}%")
    lines.append("")

    lines.append("## Corpus Statistics")
    lines.append(f"- **PUT Declarations:** {r.put_declaration_count}")
    lines.append(f"- **Test Declarations:** {r.test_declaration_count}")
    lines.append("")

    lines.extend(_render_uncovered(r))

    if r.topological_coverage is not None:
        lines.extend(_render_topological_coverage(r.topological_coverage))

    return "\n".join(lines)
