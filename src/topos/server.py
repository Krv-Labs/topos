import os
from importlib.metadata import PackageNotFoundError, version
from pathlib import Path
from typing import Any

from fastmcp import FastMCP

from topos.core.morphism import ProgramMorphism
from topos.logic.omega import ClassificationResult, SubobjectClassifier
from topos.logic.policies.base import Priority
from topos.metrics.ast.complexity import calculate_function_complexities
from topos.metrics.ast.entropy import calculate_entropy_detailed
from topos.metrics.distance import calculate_ast_distance

try:
    __version__ = version("topos")
except PackageNotFoundError:
    __version__ = "dev"

mcp = FastMCP("topos", version=__version__)

FILE_ACCESS_ROOT = Path(os.getenv("TOPOS_MCP_FILE_ROOT", Path.cwd())).resolve()


def _parse_priority(priority: str) -> Priority:
    """Parse a priority string, defaulting to BALANCED on invalid input."""
    try:
        return Priority(priority)
    except ValueError:
        return Priority.BALANCED


def _build_evaluation_response(result: ClassificationResult) -> dict[str, Any]:
    """Build the standard evaluation response dict from a ClassificationResult."""
    summary = result.summary()
    return {
        "is_parseable": result.is_parseable,
        "lattice_element": summary.name,
        "lattice_symbol": summary.symbol,
        "lattice_description": summary.description,
        "dimensions": {dim: val.name for dim, val in result.dimensions.items()},
        "dimension_symbols": {dim: val.symbol for dim, val in result.dimensions.items()},
        "scores": {
            dim: round(score * 100.0, 1)
            for dim, score in result.scores.items()
        },
        "priority": result.priority.value,
        "guidance": _build_guidance(result),
        "raw_metrics": result.raw_metrics,
    }


def _build_guidance(result: ClassificationResult) -> str:
    """Return a short, priority-aware improvement hint."""
    priority = result.priority
    s_score = result.scores.get("structural", None)
    c_score = result.scores.get("coupling", None)

    if priority == Priority.COMPOSABLE:
        if c_score is None:
            return "Coupling not measured — provide a DependencyGraph for COMPOSABLE evaluation."
        if c_score < 0.6:
            return "Reduce coupling count and balance instability toward 0.3–0.7 to achieve COMPOSABLE."
        return "COMPOSABLE target achieved. Consider structural improvements to reach SOUND."

    if priority == Priority.SELF_CONTAINED:
        if s_score is not None and s_score < 0.6:
            return "Reduce cyclomatic complexity and normalize entropy toward 0.5 to achieve SELF_CONTAINED."
        return "SELF_CONTAINED target achieved. Consider coupling improvements to reach SOUND."

    # BALANCED
    hints = []
    if s_score is not None and s_score < 0.6:
        hints.append("reduce complexity/entropy (structural)")
    if c_score is not None and c_score < 0.6:
        hints.append("reduce coupling (composability)")
    if hints:
        return "To improve: " + " and ".join(hints) + "."
    return "Code meets balanced quality targets."


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
def evaluate_code(
    code: str,
    language: str = "python",
    priority: str = "balanced",
) -> dict[str, Any]:
    """
    Evaluate code quality directly from a string.

    Classifies the code on the diamond evaluation lattice:
        BROKEN (⊥) — fails both targets
        COMPOSABLE  — good coupling; composes well with other modules
        SELF_CONTAINED — good structure; stands alone cleanly
        SOUND (⊤)  — both targets achieved

    Args:
        code:     The raw source code to evaluate.
        language: The programming language (default: 'python').
        priority: Optimization priority — shifts metric weights within each
                  dimension. One of: 'balanced' (default), 'composable',
                  'self_contained'.

    Returns:
        A dictionary with lattice_element, per-dimension scores (0–100%),
        a guidance hint, and raw metrics.
    """
    classifier = SubobjectClassifier()
    try:
        morphism = ProgramMorphism(source=code, language=language)
        result = classifier.classify_detailed(morphism, priority=_parse_priority(priority))
        return _build_evaluation_response(result)
    except Exception as e:
        return {"error": str(e)}


@mcp.tool()
def evaluate_file(filepath: str, priority: str = "balanced") -> dict[str, Any]:
    """
    Evaluate code quality of a file.

    Args:
        filepath: The path to the Python file to evaluate.
        priority: Optimization priority — 'balanced', 'composable', or
                  'self_contained'.
    """
    source, error = _read_safe_utf8_file(filepath)
    if error:
        return error
    return evaluate_code(source, priority=priority)


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
    priority: str = "balanced",
) -> dict[str, Any]:
    """
    Assess if the proposed code is an improvement over the current code.

    Compares both the lattice element and per-dimension quality scores to
    detect improvements even when the overall lattice element does not change.

    Args:
        current_code:  The existing source code.
        proposed_code: The new or refactored code.
        language:      The programming language (default: 'python').
        priority:      Optimization priority — 'balanced', 'composable', or
                       'self_contained'.

    Returns:
        A comparative analysis including status, scores, and lattice positions.
    """
    classifier = SubobjectClassifier()
    lattice = classifier.omega
    parsed_priority = _parse_priority(priority)

    try:
        curr_morph = ProgramMorphism(source=current_code, language=language)
        prop_morph = ProgramMorphism(source=proposed_code, language=language)

        curr_res = classifier.classify_detailed(curr_morph, priority=parsed_priority)
        prop_res = classifier.classify_detailed(prop_morph, priority=parsed_priority)

        curr_summary = curr_res.summary()
        prop_summary = prop_res.summary()

        is_changed_evaluation = curr_summary != prop_summary
        is_improvement = (
            lattice.leq(curr_summary, prop_summary) and is_changed_evaluation
        )
        is_regression = (
            lattice.leq(prop_summary, curr_summary) and is_changed_evaluation
        )

        curr_complexity = curr_res.raw_metrics.get("ast.complexity", 0.0)
        prop_complexity = prop_res.raw_metrics.get("ast.complexity", 0.0)
        complexity_delta = prop_complexity - curr_complexity

        # Score deltas per dimension (positive = improvement)
        score_deltas = {
            dim: round((prop_res.scores.get(dim, 0.0) - curr_res.scores.get(dim, 0.0)) * 100.0, 1)
            for dim in set(curr_res.scores) | set(prop_res.scores)
        }
        score_improved = any(d > 0 for d in score_deltas.values())
        score_regressed = any(d < 0 for d in score_deltas.values())

        can_compute_distance = curr_res.is_parseable and prop_res.is_parseable
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
        elif curr_summary == prop_summary:
            if score_improved and not score_regressed:
                status = "IMPROVEMENT (Score)"
            elif score_regressed and not score_improved:
                status = "REGRESSION (Score)"
            elif complexity_delta < 0:
                status = "IMPROVEMENT (Lower Complexity)"
            elif complexity_delta > 0:
                status = "REGRESSION (Higher Complexity)"

        return {
            "status": status,
            "priority": priority,
            "current": {
                "lattice_element": curr_summary.name,
                "lattice_symbol": curr_summary.symbol,
                "dimensions": {dim: val.name for dim, val in curr_res.dimensions.items()},
                "scores": {dim: round(s * 100.0, 1) for dim, s in curr_res.scores.items()},
                "complexity": curr_complexity,
            },
            "proposed": {
                "lattice_element": prop_summary.name,
                "lattice_symbol": prop_summary.symbol,
                "dimensions": {dim: val.name for dim, val in prop_res.dimensions.items()},
                "scores": {dim: round(s * 100.0, 1) for dim, s in prop_res.scores.items()},
                "complexity": prop_complexity,
            },
            "analysis": {
                "evaluation_improved": is_improvement,
                "evaluation_regressed": is_regression,
                "score_deltas": score_deltas,
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
def inspect_code(
    code: str,
    language: str = "python",
    priority: str = "balanced",
) -> dict[str, Any]:
    """
    Inspect detailed metrics for a code string.

    Args:
        code:     The source code to inspect.
        language: The programming language (default: 'python').
        priority: Optimization priority — 'balanced', 'composable', or
                  'self_contained'.
    """
    try:
        morphism = ProgramMorphism(source=code, language=language)
        classifier = SubobjectClassifier()
        result = classifier.classify_detailed(morphism, priority=_parse_priority(priority))

        inspection: dict[str, Any] = {
            "is_parseable": result.is_parseable,
            "lattice_element": result.summary().name,
            "lattice_symbol": result.summary().symbol,
            "dimensions": {dim: val.name for dim, val in result.dimensions.items()},
            "scores": {
                dim: round(s * 100.0, 1) for dim, s in result.scores.items()
            },
            "priority": priority,
            "guidance": _build_guidance(result),
            "raw_metrics": result.raw_metrics,
            "interpretation": result.interpretation,
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
