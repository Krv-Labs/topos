"""
CLI Entry Point
---------------
The command-line interface for topos.

This is the literal implementation of the classification map. When a user
runs `topos evaluate my_code.py`, the library:
1. Lifts the text into a Morphism (Category Object)
2. Passes it through the Subobject Classifier (Ω)
3. Outputs per-dimension Evaluations from the Lattice

Usage:
    topos evaluate path/to/code.py
    topos evaluate src/ --recursive
    topos compare file1.py file2.py
"""

from __future__ import annotations

import importlib.metadata
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import click

from topos import __version__
from topos.core.morphism import ProgramMorphism
from topos.logic.lattice import EvaluationValue
from topos.logic.omega import ClassificationResult, SubobjectClassifier


class DepgraphLoadError(RuntimeError):
    """Raised when a requested dependency graph representation cannot be built."""


@click.group()
@click.version_option(version=__version__, prog_name="topos")
def cli() -> None:
    """
    Topos: Category-theoretic code quality evaluation.

    Treating programs as morphisms in a world of structured code.
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
@click.option(
    "--gitnexus-dir",
    type=click.Path(exists=True, file_okay=False),
    default=None,
    help=(
        "Path to a .gitnexus/ directory for dependency-graph evaluation. "
        "Requires GitNexus (npm install -g gitnexus) — run "
        "'gitnexus analyze' in the repo root to generate this directory."
    ),
)
def evaluate(
    paths: tuple[str, ...],
    recursive: bool,
    verbose: bool,
    output_json: bool,
    gitnexus_dir: str | None,
) -> None:
    """
    Evaluate code quality using the Subobject Classifier.

    Analyzes Python files and classifies them per quality dimension:

    \b
    structural — Internal code structure (complexity, entropy):
      ⊥ BROKEN      - Structurally broken; cannot be evaluated
      ○ ENTANGLED   - Extreme structural or coupling pathology
      ◑ COUPLED     - Significant anomaly; tight coupling or brittle structure
      ◒ COMPLEX     - More complex than the task warrants
      ◐ STABLE      - Working code; structurally sound with minor concerns
      ⊤ SOUND       - Clean, maintainable, appropriately scoped

    \b
    coupling — Module positioning (requires --gitnexus-dir):
      Same scale applied to coupling and instability metrics.

    Examples:

    \b
        topos evaluate script.py
        topos evaluate src/ -r
        topos evaluate *.py -v
        topos evaluate src/ -r --gitnexus-dir .gitnexus
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
        try:
            result = _evaluate_file(filepath, classifier, verbose, gitnexus_dir)
        except DepgraphLoadError as exc:
            click.echo(f"Error: {exc}", err=True)
            sys.exit(1)
        results.append(result)

    if output_json:
        _output_json(results)
    else:
        _output_text(results, verbose)

    # Per-dimension overall rollup
    classification_results = [r["_result"] for r in results]
    overall = classifier.combine_dimensions(classification_results)
    click.echo()
    click.echo("Overall:")
    for dim, val in overall.items():
        click.echo(f"  {dim}: {val}")


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
@click.option(
    "--gitnexus-dir",
    type=click.Path(exists=True, file_okay=False),
    default=None,
    help=(
        "Path to a .gitnexus/ directory for dependency-graph metrics. "
        "Requires GitNexus (npm install -g gitnexus) — run "
        "'gitnexus analyze' in the repo root to generate this directory."
    ),
)
def inspect(path: str, gitnexus_dir: str | None) -> None:
    """
    Inspect detailed metrics for a single file.

    Shows all available metrics and per-dimension classification details.

    Example:

    \b
        topos inspect module.py
        topos inspect module.py --gitnexus-dir .gitnexus
    """
    from topos.metrics.ast.complexity import calculate_function_complexities
    from topos.metrics.ast.entropy import calculate_entropy_detailed

    morphism = ProgramMorphism.from_file(path)
    try:
        representations = _build_representations(str(path), gitnexus_dir)
    except DepgraphLoadError as exc:
        click.echo(f"Error: {exc}", err=True)
        sys.exit(1)

    classifier = SubobjectClassifier()
    result = classifier.classify_detailed(morphism, representations=representations)

    click.echo(f"File: {path}")
    click.echo()

    click.echo("Classification")
    click.echo("-" * 40)
    if not result.is_parseable:
        click.echo("⊥ BROKEN — parse failure")
        sys.exit(1)

    for dim, val in result.dimensions.items():
        click.echo(f"  {dim}: {val}")
    click.echo(f"  Valid Syntax: {result.is_parseable}")
    click.echo()

    click.echo("Raw Metrics")
    click.echo("-" * 40)
    for k, v in result.raw_metrics.items():
        interp = result.interpretation.get(k, "")
        suffix = f"  ({interp})" if interp else ""
        click.echo(f"  {k}: {v:.3f}{suffix}")

    if morphism.ast:
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
        click.echo(
            "Could not determine a managed installer provenance record.",
            err=True,
        )
        click.echo("If installed via pip: pip uninstall topos", err=True)
        click.echo("If installed via uv: uv pip uninstall topos", err=True)
        sys.exit(1)

    install_path = provenance.get("install_path", "").strip()
    if not install_path:
        click.echo("Installer provenance is missing install_path.", err=True)
        sys.exit(1)

    path = Path(install_path).expanduser()

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
            if not (path.is_file() or path.is_symlink()):
                click.echo(f"Refusing to remove non-file path: {path}", err=True)
                sys.exit(1)

            try:
                path.unlink()
            except OSError as exc:
                click.echo(f"Failed to remove binary {path}: {exc}", err=True)
                sys.exit(1)
            else:
                click.echo(f"Removed binary: {path}")
        else:
            click.echo(f"Binary already removed: {path}")

    if not dry_run:
        _remove_provenance_record()

    if prune_path_hints:
        _prune_path_hints(provenance, dry_run=dry_run)
    else:
        path_hint_file = provenance.get("path_hint_file", "").strip()
        if path_hint_file:
            click.echo(
                "PATH hints were left unchanged. Re-run with --prune-path-hints "
                "to remove installer-added PATH blocks."
            )


@cli.group()
def depgraph() -> None:
    """Commands for working with dependency graphs."""
    pass


@depgraph.command()
@click.option(
    "--dir",
    "directory",
    type=click.Path(exists=True, file_okay=False),
    default=None,
    help="Repository root to analyze (default: current working directory).",
)
def generate(directory: str | None) -> None:
    """
    Generate a dependency graph using GitNexus.

    Shells out to 'gitnexus analyze' and writes the .gitnexus/ directory
    that --gitnexus-dir consumes.

    Example:

    \b
        topos depgraph generate
        topos depgraph generate --dir /path/to/repo
    """
    target_dir = Path(directory).resolve() if directory else Path.cwd()

    if shutil.which("gitnexus") is None:
        click.echo(
            "GitNexus not found. Install it with: npm install -g gitnexus",
            err=True,
        )
        sys.exit(1)

    click.echo(
        "Using GitNexus (https://github.com/abhigyanpatwari/GitNexus) "
        "to generate dependency graph...\n"
    )
    click.echo("  $ gitnexus analyze\n")

    proc = subprocess.run(["gitnexus", "analyze"], cwd=target_dir)

    if proc.returncode != 0:
        sys.exit(proc.returncode)

    gitnexus_path = target_dir / ".gitnexus"
    click.echo(f"\nDependency graph written to {gitnexus_path}")
    click.echo(f"Next: topos evaluate src/ -r --gitnexus-dir {gitnexus_path}")


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


def _build_representations(filepath: str, gitnexus_dir: str | None) -> list:
    """Build extra representations for a file."""
    representations: list = []
    if gitnexus_dir:
        from topos.graphs.depgraph.graph import DependencyGraph

        try:
            depgraph = DependencyGraph.from_gitnexus_dir(gitnexus_dir, filepath)
        except (OSError, json.JSONDecodeError, ValueError, KeyError) as exc:
            raise DepgraphLoadError(
                f"Failed to build depgraph for {filepath} using {gitnexus_dir}: {exc}"
            ) from exc
        representations.append(depgraph)
    return representations


def _evaluate_file(
    filepath: Path,
    classifier: SubobjectClassifier,
    verbose: bool,
    gitnexus_dir: str | None = None,
) -> dict:
    """Evaluate a single file and return results as a dict."""
    try:
        morphism = ProgramMorphism.from_file(filepath)
        representations = _build_representations(str(filepath), gitnexus_dir)
        result = classifier.classify_detailed(morphism, representations=representations)

        complexity = result.raw_metrics.get("ast.complexity", 0.0)
        entropy = result.raw_metrics.get("ast.entropy", 0.0)

        return {
            "file": str(filepath),
            "is_parseable": result.is_parseable,
            "dimensions": {dim: val.name for dim, val in result.dimensions.items()},
            "dimension_symbols": {dim: val.symbol for dim, val in result.dimensions.items()},
            "summary": result.summary().name,
            "summary_symbol": result.summary().symbol,
            "raw_metrics": result.raw_metrics,
            # Legacy scalar fields for backward compat
            "evaluation": result.summary().name,
            "symbol": result.summary().symbol,
            "complexity": normalize_complexity(int(complexity)) if complexity else 0.0,
            "entropy": entropy,
            "valid": result.is_parseable,
            "_result": result,  # internal; stripped before JSON output
        }
    except DepgraphLoadError:
        raise
    except Exception as e:
        return {
            "file": str(filepath),
            "is_parseable": False,
            "dimensions": {},
            "dimension_symbols": {},
            "summary": "BROKEN",
            "summary_symbol": "⊥",
            "raw_metrics": {},
            "evaluation": "BROKEN",
            "symbol": "⊥",
            "complexity": 1.0,
            "entropy": 1.0,
            "valid": False,
            "error": str(e),
            "_result": ClassificationResult(is_parseable=False),
        }


def _output_text(results: list[dict], verbose: bool) -> None:
    """Output results as formatted text."""
    for result in results:
        click.echo(result["file"])
        for dim, name in result["dimensions"].items():
            sym = result["dimension_symbols"].get(dim, "")
            click.echo(f"  {dim}: {sym} {name}")
        if not result["dimensions"]:
            click.echo(f"  ⊥ BROKEN (parse failure)")

        if verbose:
            for k, v in result["raw_metrics"].items():
                click.echo(f"    {k}: {v:.3f}")
            if "error" in result:
                click.echo(f"    Error: {result['error']}")


def _output_json(results: list[dict]) -> None:
    """Output results as JSON."""
    import json

    # Strip internal _result key before serialising
    serialisable = [
        {k: v for k, v in r.items() if k != "_result"}
        for r in results
    ]
    output = {
        "version": __version__,
        "results": serialisable,
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
        key = key.strip()
        value = value.strip()
        if not key:
            continue
        data[key] = value
    return data or None


def _detect_install_method() -> tuple[str, dict[str, str] | None, str | None]:
    provenance = _load_provenance()
    if provenance and provenance.get("install_method") == "binary-installer":
        return "binary-installer", provenance, None

    try:
        dist = importlib.metadata.distribution("topos")
    except importlib.metadata.PackageNotFoundError:
        return "unknown", None, None

    try:
        installer_raw = dist.read_text("INSTALLER")
    except (FileNotFoundError, OSError, UnicodeError):
        installer_raw = "pip"

    installer = (installer_raw or "").strip().lower()
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
    rc_path = Path(path_hint_file).expanduser()

    if not rc_path.exists():
        click.echo(f"PATH hint file already absent: {rc_path}")
        return

    original_content = rc_path.read_text(encoding="utf-8")
    had_trailing_newline = original_content.endswith("\n")
    original_lines = original_content.splitlines()
    begin_index = None
    end_index = None

    for idx, line in enumerate(original_lines):
        stripped = line.strip()
        if stripped == marker_begin and begin_index is None:
            begin_index = idx
            continue
        if stripped == marker_end and begin_index is not None and end_index is None:
            end_index = idx
            break

    if begin_index is None:
        click.echo(f"No installer PATH hint block found in {rc_path}")
        return

    if end_index is None:
        click.echo(
            f"Malformed PATH hint block in {rc_path}: missing end marker {marker_end}"
        )
        return

    removed_lines = end_index - begin_index + 1
    updated_lines = original_lines[:begin_index] + original_lines[end_index + 1 :]

    if dry_run:
        click.echo(
            f"[dry-run] Would prune {removed_lines} PATH hint lines in {rc_path}"
        )
        return

    new_content = "\n".join(updated_lines)
    if had_trailing_newline:
        new_content += "\n"
    rc_path.write_text(new_content, encoding="utf-8")
    click.echo(f"Pruned installer PATH hints from {rc_path}")


def _remove_provenance_record() -> None:
    provenance_path = _provenance_file()
    try:
        provenance_path.unlink()
    except FileNotFoundError:
        return
    except OSError as exc:
        click.echo(
            f"Failed to remove provenance file {provenance_path}: {exc}", err=True
        )
        return
    click.echo(f"Removed provenance record: {provenance_path}")


def normalize_complexity(raw: int) -> float:
    from topos.logic.policies import normalize_complexity as _norm
    return _norm(raw)


if __name__ == "__main__":
    main()
