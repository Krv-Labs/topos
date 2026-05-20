"""
Coverage tools — structural test coverage (UAST).
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.functors.profunctors.uast.structural_test_coverage import (
    declaration_coverage,
)

from ..schemas import CalculateCoverageInput, CoverageResult, ResponseFormat
from ..security import resolve_within_root
from ..server import mcp

_READ_ONLY_ANN = {
    "title": "Topos Structural Coverage",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}


@mcp.tool(
    name="topos_calculate_coverage",
    tags={"coverage", "uast"},
    annotations=_READ_ONLY_ANN,
)
def topos_calculate_coverage(params: CalculateCoverageInput) -> CoverageResult:
    """Calculate structural test coverage (v2) for a set of PUT and test files.

    Uses UAST kind histograms, statement/expression recall, and k-gram path
    recall scoped to declarations. Addresses weaknesses of pooled metrics
    by using bipartite matching between PUT and test declarations.
    """
    warnings = _response_format_warnings(params.response_format)
    put_roots = []
    for path in params.put_files:
        resolved, err = resolve_within_root(path)
        if err or resolved is None:
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
                error=(
                    f"PUT file error: {(err or {}).get('error', 'path error')} "
                    f"for {path}"
                ),
            )
        try:
            morphism = ProgramMorphism.from_file(resolved, language=params.language)
            if morphism.ast and morphism.ast.uast_root:
                put_roots.append(morphism.ast.uast_root)
        except Exception as exc:
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
                error=f"Failed to parse PUT file {path}: {exc}",
            )

    test_roots = []
    for path in params.test_files:
        resolved, err = resolve_within_root(path)
        if err or resolved is None:
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
                error=(
                    f"Test file error: {(err or {}).get('error', 'path error')} "
                    f"for {path}"
                ),
            )
        try:
            morphism = ProgramMorphism.from_file(resolved, language=params.language)
            if morphism.ast and morphism.ast.uast_root:
                test_roots.append(morphism.ast.uast_root)
        except Exception as exc:
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
                error=f"Failed to parse test file {path}: {exc}",
            )

    if not put_roots:
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
            error="No valid PUT roots found (parsing failed or files empty).",
        )

    try:
        report = declaration_coverage(
            put_roots=put_roots,
            test_roots=test_roots,
            k=params.k,
            include_unknown=params.include_unknown,
        )
        return CoverageResult(
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
    except Exception as exc:
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
            error=f"Coverage calculation failed: {exc}",
        )


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

    if r.uncovered_declarations:
        lines.append("## Uncovered Declarations")
        for loc in r.uncovered_declarations:
            # Find the recall for this location
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

    return "\n".join(lines)


def _response_format_warnings(response_format: ResponseFormat) -> list[str]:
    if response_format == ResponseFormat.MARKDOWN:
        return []
    return [
        "response_format is deprecated/no-op for MCP structured output; tools return "
        "structured content regardless of this value."
    ]
