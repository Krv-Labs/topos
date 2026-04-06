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

import importlib.metadata
import os
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


@cli.command()
@click.option(
    "--dry-run",
    is_flag=True,
    help="Show what would be removed without changing anything.",
)
@click.option(
    "--yes",
    is_flag=True,
    help="Skip confirmation prompts.",
)
@click.option(
    "--prune-path-hints",
    is_flag=True,
    help="Remove PATH hint blocks previously added by the installer.",
)
def uninstall(dry_run: bool, yes: bool, prune_path_hints: bool) -> None:
    """Safely uninstall topos based on installation provenance."""
    method, provenance, uninstall_cmd = _detect_install_method()

    if method == "package-manager":
        click.echo("Detected package-manager installation.")
        click.echo(f"Run: {uninstall_cmd}")
        return

    if method != "binary-installer" or provenance is None:
        click.echo("Could not determine a managed installer provenance record.", err=True)
        click.echo("If installed via pip: pip uninstall topos", err=True)
        click.echo("If installed via uv: uv pip uninstall topos", err=True)
        sys.exit(1)

    install_path = provenance.get("install_path", "").strip()
    if not install_path:
        click.echo("Installer provenance is missing install_path.", err=True)
        sys.exit(1)

    path = Path(install_path)

    if dry_run:
        if path.exists():
            click.echo(f"[dry-run] Would remove binary: {path}")
        else:
            click.echo(f"[dry-run] Binary already removed: {path}")
    else:
        if not yes:
            confirmed = click.confirm(f"Remove binary at {path}?", default=False)
            if not confirmed:
                click.echo("Uninstall cancelled.")
                return

        if path.exists():
            path.unlink()
            click.echo(f"Removed binary: {path}")
        else:
            click.echo(f"Binary already removed: {path}")

    if prune_path_hints:
        _prune_path_hints(provenance, dry_run=dry_run)
    else:
        path_hint_file = provenance.get("path_hint_file", "").strip()
        if path_hint_file:
            click.echo(
                "PATH hints were left unchanged. Re-run with --prune-path-hints to remove "
                "installer-added PATH blocks."
            )


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


def _provenance_file() -> Path:
    override = os.environ.get("TOPOS_PROVENANCE_FILE")
    if override:
        return Path(override).expanduser()
    state_home = Path(os.environ.get("XDG_STATE_HOME", "~/.local/state")).expanduser()
    return state_home / "topos" / "install-provenance"


def _load_provenance() -> dict[str, str] | None:
    path = _provenance_file()
    if not path.exists():
        return None

    data: dict[str, str] = {}
    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key.strip()] = value.strip()
    return data or None


def _detect_install_method() -> tuple[str, dict[str, str] | None, str | None]:
    provenance = _load_provenance()
    if provenance and provenance.get("install_method") == "binary-installer":
        return "binary-installer", provenance, None

    try:
        dist = importlib.metadata.distribution("topos")
    except importlib.metadata.PackageNotFoundError:
        return "unknown", None, None

    installer = (dist.read_text("INSTALLER") or "").strip().lower()
    if installer == "uv":
        return "package-manager", None, "uv pip uninstall topos"
    if installer in {"pip", ""}:
        return "package-manager", None, "pip uninstall topos"
    return "package-manager", None, f"{installer} uninstall topos"


def _prune_path_hints(provenance: dict[str, str], dry_run: bool) -> None:
    path_hint_file = provenance.get("path_hint_file", "").strip()
    if not path_hint_file:
        click.echo("No PATH hint file recorded in installer provenance.")
        return

    marker_begin = provenance.get(
        "path_hint_begin", "# BEGIN TOPOS INSTALLER PATH"
    ).strip()
    marker_end = provenance.get("path_hint_end", "# END TOPOS INSTALLER PATH").strip()
    rc_path = Path(path_hint_file)

    if not rc_path.exists():
        click.echo(f"PATH hint file already absent: {rc_path}")
        return

    original_lines = rc_path.read_text(encoding="utf-8").splitlines()
    updated_lines: list[str] = []
    in_block = False
    removed_lines = 0

    for line in original_lines:
        stripped = line.strip()
        if stripped == marker_begin:
            in_block = True
            removed_lines += 1
            continue
        if in_block:
            removed_lines += 1
            if stripped == marker_end:
                in_block = False
            continue
        updated_lines.append(line)

    if removed_lines == 0:
        click.echo(f"No installer PATH hint block found in {rc_path}")
        return

    if dry_run:
        click.echo(f"[dry-run] Would prune {removed_lines} PATH hint lines in {rc_path}")
        return

    rc_path.write_text("\n".join(updated_lines).rstrip() + "\n", encoding="utf-8")
    click.echo(f"Pruned installer PATH hints from {rc_path}")


if __name__ == "__main__":
    main()
