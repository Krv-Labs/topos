"""
Structural comparison tools: AST edit distance between two programs.
"""

from __future__ import annotations

from topos.core.morphism import ProgramMorphism
from topos.functors.profunctors.ast.compare import calculate_ast_distance

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


@mcp.tool(
    name="topos_compare_code",
    tags={"compare"},
    annotations=_READ_ONLY_ANN,
)
def topos_compare_code(params: CompareCodeInput) -> ComparisonResult:
    """Compute AST edit distance between two source strings.

    Returns normalized distance in [0, 1] and a similarity score (1 - distance).
    Useful for detecting clones, measuring refactor impact, or — in
    conjunction with ``topos_assess_improvement`` — catching agents that
    "improve" scores without actually changing the code (near-zero distance
    + score jump = suspicious).
    """
    try:
        src = ProgramMorphism(source=params.source_code, language=params.language)
        tgt = ProgramMorphism(source=params.target_code, language=params.language)
    except Exception as exc:
        return ComparisonResult(
            raw_distance=0.0,
            normalized_distance=0.0,
            similarity=0.0,
            operations={},
            source_valid=False,
            target_valid=False,
            error=str(exc),
        )

    if not (src.is_valid and tgt.is_valid):
        return ComparisonResult(
            raw_distance=0.0,
            normalized_distance=0.0,
            similarity=0.0,
            operations={},
            source_valid=src.is_valid,
            target_valid=tgt.is_valid,
            error="Failed to parse one or both code snippets.",
        )

    result = calculate_ast_distance(src.ast, tgt.ast)
    return ComparisonResult(
        raw_distance=result.raw_distance,
        normalized_distance=result.normalized_distance,
        similarity=1.0 - result.normalized_distance,
        operations=dict(result.operations),
        source_valid=True,
        target_valid=True,
    )


@mcp.tool(
    name="topos_compare_files",
    tags={"compare"},
    annotations=_READ_ONLY_ANN,
)
def topos_compare_files(params: CompareFilesInput) -> ComparisonResult:
    """AST distance between two files on disk."""
    source_text, source_err = read_safe_utf8_file(params.source)
    if source_err:
        return ComparisonResult(
            raw_distance=0.0,
            normalized_distance=0.0,
            similarity=0.0,
            operations={},
            source_valid=False,
            target_valid=False,
            error=f"Source file error: {source_err['error']}",
        )
    target_text, target_err = read_safe_utf8_file(params.target)
    if target_err:
        return ComparisonResult(
            raw_distance=0.0,
            normalized_distance=0.0,
            similarity=0.0,
            operations={},
            source_valid=True,
            target_valid=False,
            error=f"Target file error: {target_err['error']}",
        )
    return topos_compare_code(
        CompareCodeInput(
            source_code=source_text or "",
            target_code=target_text or "",
            response_format=params.response_format,
        )
    )
