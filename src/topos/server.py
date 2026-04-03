import os
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path
from typing import Any

from fastmcp import FastMCP

from topos.core.morphism import ProgramMorphism
from topos.logic.omega import SubobjectClassifier
from topos.metrics.complexity import calculate_function_complexities
from topos.metrics.distance import calculate_ast_distance
from topos.metrics.entropy import calculate_entropy_detailed

try:
    __version__ = version("topos")
except PackageNotFoundError:
    __version__ = "dev"

mcp = FastMCP("topos", version=__version__)

FILE_ACCESS_ROOT = Path(os.getenv("TOPOS_MCP_FILE_ROOT", Path.cwd())).resolve()


def _is_within_allowed_root(path: Path) -> bool:
    """Return whether a path is under the configured file access root."""
    try:
        path.relative_to(FILE_ACCESS_ROOT)
        return True
    except ValueError:
        return False


def _read_safe_utf8_file(filepath: str) -> tuple[str | None, dict[str, str] | None]:
    """Read a UTF-8 file if it is within the allowed root and safe to access."""
    path = Path(filepath)

    try:
        resolved_path = path.resolve(strict=False)
    except OSError:
        return None, {"error": f"Invalid path: {filepath}"}

    if not _is_within_allowed_root(resolved_path):
        return None, {"error": f"Access denied: path must be under {FILE_ACCESS_ROOT}"}

    try:
        return resolved_path.read_text(encoding="utf-8"), None
    except FileNotFoundError:
        return None, {"error": f"File not found: {filepath}"}
    except IsADirectoryError:
        return None, {"error": f"Path is not a file: {filepath}"}
    except UnicodeDecodeError:
        return None, {"error": f"File is not valid UTF-8 text: {filepath}"}
    except OSError as exc:
        return None, {"error": f"Unable to read file '{filepath}': {exc}"}


@mcp.tool()
def evaluate_code(code: str, language: str = "python") -> dict[str, Any]:
    """
    Evaluate code quality directly from a string.
    Analyzes the code and classifies it in the evaluation lattice.

    Args:
        code: The raw source code to evaluate.
        language: The programming language (default: 'python').

    Returns:
        A dictionary containing the evaluation result,
        metrics, and classification symbol.
    """
    classifier = SubobjectClassifier()
    try:
        morphism = ProgramMorphism(source=code, language=language)
        result = classifier.classify_detailed(morphism)

        return {
            "evaluation": result.evaluation.name,
            "symbol": result.evaluation.symbol,
            "description": result.evaluation.description,
            "complexity_score": result.complexity_score,
            "entropy_score": result.entropy_score,
            "is_valid": result.is_valid,
            "metrics": result.metrics,
        }
    except Exception as e:
        return {"error": str(e)}


@mcp.tool()
def evaluate_file(filepath: str) -> dict[str, Any]:
    """
    Evaluate code quality of a file.

    Args:
        filepath: The path to the Python file to evaluate.
    """
    source, error = _read_safe_utf8_file(filepath)
    if error:
        return error
    return evaluate_code(source)


@mcp.tool()
def compare_code(
    source_code: str,
    target_code: str,
    language: str = "python",
) -> dict[str, Any]:
    """
    Compare structural distance between two code strings.
    Computes the AST edit distance (topological drift).

    Args:
        source_code: The source code string.
        target_code: The target code string (e.g., a proposed refactor).
        language: The programming language (default: 'python').

    Returns:
        A dictionary containing distance metrics and edit operations.
    """
    try:
        source_morph = ProgramMorphism(source=source_code, language=language)
        target_morph = ProgramMorphism(source=target_code, language=language)
        source_valid = source_morph.is_valid
        target_valid = target_morph.is_valid

        if not source_valid or not target_valid:
            return {
                "error": "Failed to parse one or both code snippets.",
                "source_valid": source_valid,
                "target_valid": target_valid,
            }

        result = calculate_ast_distance(source_morph.ast, target_morph.ast)

        return {
            "raw_distance": result.raw_distance,
            "normalized_distance": result.normalized_distance,
            "similarity": 1.0 - result.normalized_distance,
            "operations": result.operations,
            "source_valid": source_valid,
            "target_valid": target_valid,
        }
    except Exception as e:
        return {"error": str(e)}


@mcp.tool()
def compare_files(source: str, target: str) -> dict[str, Any]:
    """
    Compare structural distance between two files.
    """
    source_text, source_error = _read_safe_utf8_file(source)
    if source_error:
        return {"error": f"Source file error: {source_error['error']}"}

    target_text, target_error = _read_safe_utf8_file(target)
    if target_error:
        return {"error": f"Target file error: {target_error['error']}"}

    return compare_code(
        source_text,
        target_text,
    )


@mcp.tool()
def assess_improvement(
    current_code: str,
    proposed_code: str,
    language: str = "python",
) -> dict[str, Any]:
    """
    Assess if the proposed code is an improvement over the current code.
    Uses the topos evaluation lattice and structural metrics to determine if the
    change improves quality, reduces complexity, or maintainability.

    Args:
        current_code: The existing source code.
        proposed_code: The new or refactored code.
        language: The programming language (default: 'python').

    Returns:
        A comparative analysis of the improvement.
    """
    classifier = SubobjectClassifier()
    lattice = classifier.omega

    try:
        curr_morph = ProgramMorphism(source=current_code, language=language)
        prop_morph = ProgramMorphism(source=proposed_code, language=language)

        curr_res = classifier.classify_detailed(curr_morph)
        prop_res = classifier.classify_detailed(prop_morph)

        is_changed_evaluation = curr_res.evaluation != prop_res.evaluation
        is_improvement = (
            lattice.leq(curr_res.evaluation, prop_res.evaluation)
            and is_changed_evaluation
        )
        is_regression = (
            lattice.leq(prop_res.evaluation, curr_res.evaluation)
            and is_changed_evaluation
        )

        complexity_delta = prop_res.complexity_score - curr_res.complexity_score

        can_compute_distance = curr_res.is_valid and prop_res.is_valid
        dist_res = (
            calculate_ast_distance(curr_morph.ast, prop_morph.ast)
            if can_compute_distance
            else None
        )

        status = "LATERAL_MOVE"
        if is_improvement:
            status = "IMPROVEMENT"
        elif is_regression:
            status = "REGRESSION"
        elif curr_res.evaluation == prop_res.evaluation:
            if complexity_delta < 0:
                status = "IMPROVEMENT (Lower Complexity)"
            elif complexity_delta > 0:
                status = "REGRESSION (Higher Complexity)"

        return {
            "status": status,
            "current": {
                "evaluation": curr_res.evaluation.name,
                "symbol": curr_res.evaluation.symbol,
                "complexity": curr_res.complexity_score,
            },
            "proposed": {
                "evaluation": prop_res.evaluation.name,
                "symbol": prop_res.evaluation.symbol,
                "complexity": prop_res.complexity_score,
            },
            "analysis": {
                "evaluation_improved": is_improvement,
                "evaluation_regressed": is_regression,
                "complexity_delta": complexity_delta,
                "distance_computed": can_compute_distance,
                "structural_distance": (
                    dist_res.normalized_distance if dist_res is not None else None
                ),
                "similarity": (
                    1.0 - dist_res.normalized_distance if dist_res is not None else None
                ),
            },
        }
    except Exception as e:
        return {"error": str(e)}


@mcp.tool()
def inspect_code(code: str, language: str = "python") -> dict[str, Any]:
    """
    Inspect detailed metrics for a code string.

    Args:
        code: The source code to inspect.
        language: The programming language (default: 'python').
    """
    try:
        morphism = ProgramMorphism(source=code, language=language)
        classifier = SubobjectClassifier()
        result = classifier.classify_detailed(morphism)

        inspection: dict[str, Any] = {
            "evaluation": result.evaluation.name,
            "symbol": result.evaluation.symbol,
            "is_valid": result.is_valid,
            "complexity_score": result.complexity_score,
            "entropy_score": result.entropy_score,
            "ast_metrics": result.metrics,
            "functions": {},
            "entropy_details": {},
        }

        if morphism.ast:
            func_complexities = calculate_function_complexities(morphism.ast)
            if func_complexities:
                inspection["functions"] = func_complexities

        entropy = calculate_entropy_detailed(morphism.source)
        inspection["entropy_details"] = {
            "compression_ratio": entropy.ratio,
            "interpretation": entropy.interpretation,
        }

        return inspection
    except Exception as e:
        return {"error": str(e)}


def main() -> None:
    """Entry point for the MCP server."""
    mcp.run()


if __name__ == "__main__":
    main()
