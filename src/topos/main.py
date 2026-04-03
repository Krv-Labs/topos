"""
CLI Entry Point
---------------
The command-line interface for topos.

This is the literal implementation of the classification map. When a user
runs `topos evaluate my_code.py`, the library:
1. Lifts the text into a Morphism (Category Object)
2. Passes it through the Subobject Classifier (Ω)
3. Outputs an Evaluation from the Lattice

Usage:
    topos evaluate path/to/code.py
    topos evaluate src/ --recursive
    topos compare file1.py file2.py
"""

from __future__ import annotations

import sys
from pathlib import Path

import click

from topos import __version__
from topos.core.morphism import ProgramMorphism
from topos.logic.lattice import EvaluationValue
from topos.logic.omega import SubobjectClassifier


@click.group()
@click.version_option(version=__version__, prog_name="topos")
def cli() -> None:
    """
    Topos: Category-theoretic code quality evaluation.

    Treating programs as morphisms in a world of commodity code.
    Building the subobject classifier for rigorous program evaluations.
    """
    pass


@cli.command()
@click.argument("paths", nargs=-1, type=click.Path(exists=True))
@click.option(
    "-r",
    "--recursive",
    is_flag=True,
    help="Recursively evaluate directories.",
)
@click.option(
    "-v",
    "--verbose",
    is_flag=True,
    help="Show detailed metrics for each file.",
)
@click.option(
    "--json",
    "output_json",
    is_flag=True,
    help="Output results as JSON.",
)
def evaluate(
    paths: tuple[str, ...],
    recursive: bool,
    verbose: bool,
    output_json: bool,
) -> None:
    """
    Evaluate code quality using the Subobject Classifier.

    Analyzes Python files and classifies them in the evaluation lattice:

    \b
    ⊥ INVALID        - Syntactically broken code
    ○ HALLUCINATED   - Likely vacuous or fabricated output
    ◑ NOISY         - Repetitive/atypical structural signal
    ◒ WEAK          - Functional with elevated structural risk
    ◐ COMMODITY     - Functional but with concerns
    ⊤ VERIFIED       - Maintainable and human-aligned

    Examples:

    \b
        topos evaluate script.py
        topos evaluate src/ -r
        topos evaluate *.py -v
    """
    if not paths:
        click.echo("Error: No paths provided.", err=True)
        sys.exit(1)

    files = _collect_files(paths, recursive)

    if not files:
        click.echo("No Python files found.", err=True)
        sys.exit(1)

    classifier = SubobjectClassifier()
    results: list[dict] = []

    for filepath in files:
        result = _evaluate_file(filepath, classifier, verbose)
        results.append(result)

    if output_json:
        _output_json(results)
    else:
        _output_text(results, verbose)

    overall = classifier.combine(*[EvaluationValue[r["evaluation"]] for r in results])
    click.echo()
    click.echo(f"Overall: {overall}")


@cli.command()
@click.argument("source", type=click.Path(exists=True))
@click.argument("target", type=click.Path(exists=True))
@click.option(
    "-v",
    "--verbose",
    is_flag=True,
    help="Show detailed distance metrics.",
)
def compare(source: str, target: str, verbose: bool) -> None:
    """
    Compare structural distance between two programs.

    Computes the AST edit distance (topological drift) between
    two Python files.

    Example:

    \b
        topos compare original.py refactored.py
    """
    from topos.metrics.distance import calculate_ast_distance

    source_morph = ProgramMorphism.from_file(source)
    target_morph = ProgramMorphism.from_file(target)

    if source_morph.ast is None or target_morph.ast is None:
        click.echo("Error: Failed to parse one or both files.", err=True)
        sys.exit(1)

    result = calculate_ast_distance(source_morph.ast, target_morph.ast)

    click.echo(f"Source: {source}")
    click.echo(f"Target: {target}")
    click.echo()
    click.echo(f"Edit Distance: {result.raw_distance}")
    click.echo(f"Similarity: {1 - result.normalized_distance:.1%}")

    if verbose:
        click.echo()
        click.echo("Operations:")
        click.echo(f"  Insertions:    {result.operations['insertions']}")
        click.echo(f"  Deletions:     {result.operations['deletions']}")
        click.echo(f"  Substitutions: {result.operations['substitutions']}")


@cli.command()
@click.argument("path", type=click.Path(exists=True))
def inspect(path: str) -> None:
    """
    Inspect detailed metrics for a single file.

    Shows all available metrics and classification details.

    Example:

    \b
        topos inspect module.py
    """
    from topos.metrics.complexity import calculate_function_complexities
    from topos.metrics.entropy import calculate_entropy_detailed

    morphism = ProgramMorphism.from_file(path)
    classifier = SubobjectClassifier()
    result = classifier.classify_detailed(morphism)

    click.echo(f"File: {path}")
    click.echo()

    click.echo("Classification")
    click.echo("-" * 40)
    click.echo(f"Evaluation: {result.evaluation}")
    click.echo(f"Valid Syntax: {result.is_valid}")
    click.echo()

    click.echo("Metrics")
    click.echo("-" * 40)
    click.echo(f"Complexity Score: {result.complexity_score:.3f}")
    click.echo(f"Entropy Score: {result.entropy_score:.3f}")

    if morphism.ast:
        click.echo(f"AST Nodes: {result.metrics.get('node_count', 'N/A')}")
        click.echo(f"AST Depth: {result.metrics.get('depth', 'N/A')}")

        click.echo()
        click.echo("Function Complexities")
        click.echo("-" * 40)
        func_complexities = calculate_function_complexities(morphism.ast)
        if func_complexities:
            for func, complexity in sorted(
                func_complexities.items(),
                key=lambda x: x[1],
                reverse=True,
            ):
                click.echo(f"  {func}: {complexity}")
        else:
            click.echo("  (no functions defined)")

    click.echo()
    click.echo("Entropy Analysis")
    click.echo("-" * 40)
    entropy = calculate_entropy_detailed(morphism.source)
    click.echo(f"  Compression Ratio: {entropy.ratio:.3f}")
    click.echo(f"  Interpretation: {entropy.interpretation}")


def main() -> None:
    """Console script entrypoint."""
    cli()


def _collect_files(paths: tuple[str, ...], recursive: bool) -> list[Path]:
    """Collect all Python files from the given paths."""
    files: list[Path] = []

    for path_str in paths:
        path = Path(path_str)

        if path.is_file():
            if path.suffix == ".py":
                files.append(path)
        elif path.is_dir():
            pattern = "**/*.py" if recursive else "*.py"
            files.extend(path.glob(pattern))

    return sorted(set(files))


def _evaluate_file(
    filepath: Path,
    classifier: SubobjectClassifier,
    verbose: bool,
) -> dict:
    """Evaluate a single file and return results as a dict."""
    try:
        morphism = ProgramMorphism.from_file(filepath)
        result = classifier.classify_detailed(morphism)

        return {
            "file": str(filepath),
            "evaluation": result.evaluation.name,
            "symbol": result.evaluation.symbol,
            "complexity": result.complexity_score,
            "entropy": result.entropy_score,
            "valid": result.is_valid,
            "metrics": result.metrics,
        }
    except Exception as e:
        return {
            "file": str(filepath),
            "evaluation": "INVALID",
            "symbol": "⊥",
            "complexity": 1.0,
            "entropy": 1.0,
            "valid": False,
            "error": str(e),
        }


def _output_text(results: list[dict], verbose: bool) -> None:
    """Output results as formatted text."""
    for result in results:
        line = f"{result['symbol']} {result['evaluation']:12} {result['file']}"
        click.echo(line)

        if verbose:
            click.echo(f"   Complexity: {result['complexity']:.3f}")
            click.echo(f"   Entropy: {result['entropy']:.3f}")
            if "error" in result:
                click.echo(f"   Error: {result['error']}")


def _output_json(results: list[dict]) -> None:
    """Output results as JSON."""
    import json

    output = {
        "version": __version__,
        "results": results,
    }
    click.echo(json.dumps(output, indent=2))


if __name__ == "__main__":
    main()
