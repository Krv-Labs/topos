"""
Structural comparison tools: AST edit distance between two programs.
"""

from __future__ import annotations

from fastmcp.tools.base import ToolResult

from topos.core.morphism import ProgramMorphism
from topos.functors.profunctors.ast.compare import calculate_ast_distance

from ..formatting import to_tool_result
from ..schemas import (
    CompareCodeInput,
    CompareFilesInput,
    ComparisonResult,
)
from ..security import read_safe_utf8_file
from ..server import mcp

_READ_ONLY_ANN = {
    "title": "Topos Structural Comparison",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}


def _failed_comparison(
    error: str, source_valid: bool = False, target_valid: bool = False
) -> ComparisonResult:
    return ComparisonResult(
        raw_distance=0.0,
        normalized_distance=0.0,
        similarity=0.0,
        operations={},
        source_valid=source_valid,
        target_valid=target_valid,
        error=error,
    )


@mcp.tool(
    name="topos_compare_code",
    tags={"compare"},
    annotations=_READ_ONLY_ANN,
)
def topos_compare_code(params: CompareCodeInput) -> ToolResult:
    """Compute the AST (tree-edit) distance between two source-code strings.

    Read-only and idempotent; parses both snippets in memory, never writes or
    scores. Use for clone detection or to measure refactor impact; the
    ``topos_assess_*`` tools already fold this in as an anti-gaming check, so
    call it directly only for the raw number. Returns a ComparisonResult:
    ``normalized_distance`` in [0, 1], ``similarity`` (= 1 - it),
    ``raw_distance``, an ``operations`` edit-count map, and
    ``source_valid``/``target_valid`` (``error`` set if either fails to parse).
    """
    try:
        src = ProgramMorphism(source=params.source_code, language=params.language)
        tgt = ProgramMorphism(source=params.target_code, language=params.language)
    except Exception as exc:
        model = _failed_comparison(str(exc))
        return to_tool_result(model, render_comparison_md(model))

    if not (src.is_valid and tgt.is_valid):
        model = _failed_comparison(
            "Failed to parse one or both code snippets.",
            source_valid=src.is_valid,
            target_valid=tgt.is_valid,
        )
        return to_tool_result(model, render_comparison_md(model))

    result = calculate_ast_distance(src.ast, tgt.ast)
    model = ComparisonResult(
        raw_distance=result.raw_distance,
        normalized_distance=result.normalized_distance,
        similarity=1.0 - result.normalized_distance,
        operations=dict(result.operations),
        source_valid=True,
        target_valid=True,
    )
    return to_tool_result(model, render_comparison_md(model))


@mcp.tool(
    name="topos_compare_files",
    tags={"compare"},
    annotations=_READ_ONLY_ANN,
)
def topos_compare_files(params: CompareFilesInput) -> ToolResult:
    """Compute the AST (tree-edit) distance between two source files on disk.

    Read-only; parses both files, never writes or scores. Use for clone
    detection or refactor impact; use ``topos_assess_*`` for a quality verdict.
    Returns a ComparisonResult: ``normalized_distance`` in [0, 1],
    ``similarity`` (= 1 - it), ``raw_distance``, an ``operations`` edit-count
    map, and ``source_valid``/``target_valid`` (``error`` set on read/parse
    failure).
    """
    source_text, source_err = read_safe_utf8_file(params.source)
    if source_err:
        model = _failed_comparison(
            f"Source file error: {source_err['error']}",
            source_valid=False,
            target_valid=False,
        )
        return to_tool_result(model, render_comparison_md(model))
    target_text, target_err = read_safe_utf8_file(params.target)
    if target_err:
        model = _failed_comparison(
            f"Target file error: {target_err['error']}",
            source_valid=True,
            target_valid=False,
        )
        return to_tool_result(model, render_comparison_md(model))
    return topos_compare_code(
        CompareCodeInput(
            source_code=source_text or "",
            target_code=target_text or "",
        )
    )


# ---------------------------------------------------------------------------
# Markdown renderer (rendered into ToolResult.content)
# ---------------------------------------------------------------------------


def render_comparison_md(r: ComparisonResult) -> str:
    """Compact markdown for an AST-distance comparison."""
    if r.error:
        return f"**Error:** {r.error}"
    lines = [
        f"**Normalized distance:** {r.normalized_distance:.3f}",
        f"**Similarity:** {r.similarity:.3f}",
        f"**Raw distance:** {r.raw_distance:.1f}",
        f"**Validity:** source={r.source_valid}, target={r.target_valid}",
    ]
    if r.operations:
        ops = ", ".join(f"{k}={v}" for k, v in sorted(r.operations.items()))
        lines.append(f"**Operations:** {ops}")
    return "\n".join(lines)
