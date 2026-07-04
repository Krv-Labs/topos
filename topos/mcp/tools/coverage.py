"""
Coverage tools — structural test coverage (UAST).
"""

from __future__ import annotations

from fastmcp.tools.base import ToolResult

from topos.core.morphism import ProgramMorphism
from topos.functors.profunctors.uast.structural_test_coverage import (
    declaration_coverage,
)

from ..formatting import to_tool_result
from ..schemas import CalculateCoverageInput, CoverageResult
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
    """Measure how well a test suite exercises its program-under-test, via
    structural (UAST) coverage (read-only).

    A standalone signal, separate from the SIMPLE/COMPOSABLE/SECURE lattice; for
    a quality verdict use ``topos_evaluate_*`` instead. Computes UAST bipartite
    declaration matching and k-gram path recall. Returns a CoverageResult.
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

    return "\n".join(lines)
