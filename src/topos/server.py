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


@mcp.tool()
def evaluate_code(code: str, language: str = "python") -> dict[str, Any]:
    """
    Evaluate code quality directly from a string.
    Analyzes the code and classifies it in the evaluation lattice.
    
    Args:
        code: The raw source code to evaluate.
        language: The programming language (default: 'python').
        
    Returns:
        A dictionary containing the evaluation result, metrics, and classification symbol.
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
    path = Path(filepath)
    if not path.exists():
        return {"error": f"File not found: {filepath}"}
    return evaluate_code(path.read_text(encoding="utf-8"))


@mcp.tool()
def compare_code(source_code: str, target_code: str, language: str = "python") -> dict[str, Any]:
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

        if source_morph.ast is None or target_morph.ast is None:
            return {"error": "Failed to parse one or both code snippets."}

        result = calculate_ast_distance(source_morph.ast, target_morph.ast)

        return {
            "raw_distance": result.raw_distance,
            "normalized_distance": result.normalized_distance,
            "similarity": 1.0 - result.normalized_distance,
            "operations": result.operations,
        }
    except Exception as e:
        return {"error": str(e)}


@mcp.tool()
def compare_files(source: str, target: str) -> dict[str, Any]:
    """
    Compare structural distance between two files.
    """
    source_path = Path(source)
    target_path = Path(target)

    if not source_path.exists():
        return {"error": f"Source file not found: {source}"}
    if not target_path.exists():
        return {"error": f"Target file not found: {target}"}

    return compare_code(
        source_path.read_text(encoding="utf-8"),
        target_path.read_text(encoding="utf-8")
    )


@mcp.tool()
def assess_improvement(current_code: str, proposed_code: str, language: str = "python") -> dict[str, Any]:
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

        # Lattice check
        is_improvement = lattice.leq(curr_res.evaluation, prop_res.evaluation) and curr_res.evaluation != prop_res.evaluation
        is_regression = lattice.leq(prop_res.evaluation, curr_res.evaluation) and curr_res.evaluation != prop_res.evaluation
        
        # Complexity check
        complexity_delta = prop_res.complexity_score - curr_res.complexity_score
        
        # Distance check
        dist_res = calculate_ast_distance(curr_morph.ast, prop_morph.ast) if curr_morph.ast and prop_morph.ast else None

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
                "structural_distance": dist_res.normalized_distance if dist_res else 1.0,
                "similarity": 1.0 - (dist_res.normalized_distance if dist_res else 1.0),
            }
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
