"""
CLI Entry Point
---------------
The command-line interface for topos.

This is the literal implementation of the classification map. When a user
runs `topos evaluate my_code.py`, the library:
1. Lifts the text into a Morphism (Categorical Arrow)
2. Passes it through the Subobject Classifier (Ω)
3. Outputs per-dimension Evaluations from the Lattice

Usage:
    topos evaluate path/to/code.py
    topos evaluate src/ --recursive
    topos compare file1.py file2.py
    topos structural-test-coverage --tests t.py src/m.py
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
from topos.graphs.ast.dispatch import SUPPORTED_LANGUAGES, language_file_suffixes
from topos.logic.omega import ClassificationResult, SubobjectClassifier
from topos.logic.policies.base import Priority

_EVALUATE_LANGUAGE_CHOICE = click.Choice(sorted(SUPPORTED_LANGUAGES))


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
    "--priority",
    type=click.Choice(["balanced", "composable", "self_contained"]),
    default="balanced",
    show_default=True,
    help="Optimization priority: shifts metric weights toward the selected target.",
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
@click.option(
    "--language",
    type=_EVALUATE_LANGUAGE_CHOICE,
    default="python",
    show_default=True,
    help="Source language for parsing and file discovery when paths are directories.",
)
def evaluate(
    paths: tuple[str, ...],
    recursive: bool,
    verbose: bool,
    output_json: bool,
    priority: str,
    gitnexus_dir: str | None,
    language: str,
) -> None:
    """
    Evaluate code quality using the Subobject Classifier.

    Classifies files on the diamond evaluation lattice:

    \b
      ⊥ BROKEN         — Fails both targets (low quality or parse failure)
      ◑ COMPOSABLE     — Good coupling; composes well with other modules
      ◐ SELF_CONTAINED — Good structure; low complexity, clean entropy
      ⊤ SOUND          — Both targets achieved (the ideal)

    \b
    Use --priority to direct the evaluation toward a specific target:
      balanced       Equal weight on all metrics (default)
      composable     Upweights coupling quality
      self_contained Upweights structural quality (complexity + entropy)

    \b
    coupling dimension requires --gitnexus-dir (dependency graph data).

    Examples:

    \b
        topos evaluate script.py
        topos evaluate src/ -r --priority self_contained
        topos evaluate *.py -v
        topos evaluate src/ -r --gitnexus-dir .gitnexus --priority composable
    """
    if not paths:
        click.echo("Error: No paths provided.", err=True)
        sys.exit(1)

    files = _collect_files(paths, recursive, language)

    if not files:
        suffixes = ", ".join(language_file_suffixes(language))
        click.echo(
            f"No {language} source files found (expected suffixes: {suffixes}).",
            err=True,
        )
        sys.exit(1)

    classifier = SubobjectClassifier()
    results: list[dict] = []

    parsed_priority = Priority(priority)

    for filepath in files:
        try:
            result = _evaluate_file(
                filepath,
                classifier,
                verbose,
                gitnexus_dir,
                parsed_priority,
                language=language,
            )
        except DepgraphLoadError as exc:
            click.echo(f"Error: {exc}", err=True)
            sys.exit(1)
        results.append(result)

    if output_json:
        _output_json(results)
    else:
        _output_text(results, verbose)

    # Per-dimension overall rollup (min score across files, then threshold)
    classification_results = [r["_result"] for r in results]
    overall = classifier.combine_dimensions(classification_results)

    click.echo()
    click.echo("Overall:")
    if not overall:
        click.echo("  structural: ⊥ BROKEN (no evaluable dimensions)")
        return

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


@cli.command("structural-test-coverage")
@click.option(
    "--language",
    default="python",
    show_default=True,
    type=_EVALUATE_LANGUAGE_CHOICE,
    help="Language for tree-sitter / UAST parsing of all listed files.",
)
@click.option(
    "--tests",
    "test_paths",
    type=click.Path(exists=True, dir_okay=False),
    multiple=True,
    required=True,
    help="Test file path (repeat for multiple test modules).",
)
@click.option(
    "--k",
    "kgram_length",
    default=3,
    show_default=True,
    type=int,
    help="Length of each DFS kind n-gram for path recall.",
)
@click.option(
    "--include-unknown",
    is_flag=True,
    help="Count Unknown UAST kinds in histograms and k-grams.",
)
@click.option(
    "--json",
    "output_json",
    is_flag=True,
    help="Emit a single JSON object with scores and diagnostics.",
)
@click.option(
    "--v2",
    "use_v2",
    is_flag=True,
    help="Use declaration-level bipartite coverage (v2) instead of v0/v1.",
)
@click.option(
    "--coverage-threshold",
    default=0.5,
    show_default=True,
    type=float,
    help="(v2 only) Minimum best-match recall to count a PUT declaration as covered.",
)
@click.argument(
    "put_paths",
    nargs=-1,
    type=click.Path(exists=True, dir_okay=False),
    required=True,
)
def structural_test_coverage_cmd(
    put_paths: tuple[str, ...],
    test_paths: tuple[str, ...],
    language: str,
    kgram_length: int,
    include_unknown: bool,
    output_json: bool,
    use_v2: bool,
    coverage_threshold: float,
) -> None:
    """
    Structural overlap of tests toward the program-under-test (UAST).

    Default (v0/v1): kind recall, control-flow recall, and k-gram path recall
    over pooled UAST kind histograms.

    With --v2: declaration-level bipartite coverage — each PUT function/method
    is matched against test declarations by body structure recall. Includes
    precision signal, F2 score, and uncovered declaration locations.

    Example:

    \b
        topos structural-test-coverage --tests tests/test_foo.py src/foo.py
        topos structural-test-coverage --v2 --tests tests/test_foo.py src/foo.py
    """
    from dataclasses import asdict

    from topos.graphs.ast.dispatch import parse_source

    if kgram_length < 1:
        click.echo("Error: --k must be >= 1.", err=True)
        sys.exit(1)

    put_roots: list[object] = []
    for path in put_paths:
        source = Path(path).read_text(encoding="utf-8")
        result = parse_source(source=source, language=language, file=str(path))
        if result.uast_root is None:
            click.echo(f"Error: No UAST root for PUT file {path}.", err=True)
            sys.exit(1)
        put_roots.append(result.uast_root)

    test_roots: list[object] = []
    for path in test_paths:
        source = Path(path).read_text(encoding="utf-8")
        result = parse_source(source=source, language=language, file=str(path))
        if result.uast_root is None:
            click.echo(f"Error: No UAST root for test file {path}.", err=True)
            sys.exit(1)
        test_roots.append(result.uast_root)

    if use_v2:
        from topos.metrics.uast.structural_test_coverage import declaration_coverage

        report_v2 = declaration_coverage(
            put_roots,
            test_roots,
            k=kgram_length,
            include_unknown=include_unknown,
            coverage_threshold=coverage_threshold,
        )
        if output_json:
            payload = asdict(report_v2)
            payload["language"] = language
            payload["put_paths"] = list(put_paths)
            payload["test_paths"] = list(test_paths)
            click.echo(json.dumps(payload, indent=2))
            return
        _print_v2_report(report_v2, put_paths, test_paths, language)
        return

    from topos.metrics.uast.structural_test_coverage import structural_test_coverage

    report = structural_test_coverage(
        put_roots,
        test_roots,
        k=kgram_length,
        include_unknown=include_unknown,
    )

    if output_json:
        payload = asdict(report)
        payload["language"] = language
        payload["put_paths"] = list(put_paths)
        payload["test_paths"] = list(test_paths)
        click.echo(json.dumps(payload, indent=2))
        return

    click.echo("Structural test coverage (UAST)")
    click.echo(f"Language: {language}")
    click.echo(f"PUT files ({len(put_paths)}): {', '.join(put_paths)}")
    click.echo(f"Test files ({len(test_paths)}): {', '.join(test_paths)}")
    click.echo()
    click.echo("Scores (higher = more PUT structure mass matched in tests)")
    click.echo("-" * 48)
    click.echo(f"  Kind recall (v0):           {report.kind_recall:.4f}")
    click.echo(f"  Control-flow recall (v0): {report.control_flow_recall:.4f}")
    click.echo(f"  Composite v0:             {report.composite_v0:.4f}")
    click.echo(f"  Path recall k={report.k} (v1):     {report.path_recall_kgram:.4f}")
    click.echo()
    click.echo("Diagnostics")
    click.echo("-" * 48)
    click.echo(f"  PUT kind nodes (histogram):  {report.put_kind_nodes}")
    click.echo(f"  Test kind nodes (histogram): {report.test_kind_nodes}")
    click.echo(f"  PUT CF nodes:                {report.put_cf_nodes}")
    click.echo(f"  Test CF nodes:               {report.test_cf_nodes}")
    click.echo(f"  PUT k-gram mass:             {report.put_kgram_mass}")
    click.echo(f"  Test k-gram mass:            {report.test_kgram_mass}")


def _print_v2_report(
    report: object,
    put_paths: tuple[str, ...],
    test_paths: tuple[str, ...],
    language: str,
) -> None:
    click.echo("Structural test coverage v2 (declaration-level bipartite)")
    click.echo(f"Language: {language}")
    click.echo(f"PUT files ({len(put_paths)}): {', '.join(put_paths)}")
    click.echo(f"Test files ({len(test_paths)}): {', '.join(test_paths)}")
    click.echo()
    click.echo("Declaration Coverage")
    click.echo("-" * 52)
    click.echo(f"  Mean declaration coverage:  {report.mean_declaration_coverage:.4f}")  # type: ignore[attr-defined]
    click.echo(f"  Declaration coverage rate:  {report.declaration_coverage_rate:.4f}")  # type: ignore[attr-defined]
    click.echo(f"  Coverage threshold:         {report.coverage_threshold:.2f}")  # type: ignore[attr-defined]
    click.echo(f"  PUT declarations:           {report.put_declaration_count}")  # type: ignore[attr-defined]
    click.echo(f"  Test declarations:          {report.test_declaration_count}")  # type: ignore[attr-defined]
    click.echo()
    click.echo("Category-Stratified Recall (disjoint)")
    click.echo("-" * 52)
    click.echo(f"  Statement recall:           {report.stmt_recall:.4f}")  # type: ignore[attr-defined]
    click.echo(f"  Expression recall:          {report.expr_recall:.4f}")  # type: ignore[attr-defined]
    click.echo()
    click.echo("Precision and F-score")
    click.echo("-" * 52)
    click.echo(f"  Mean test precision:        {report.mean_test_precision:.4f}")  # type: ignore[attr-defined]
    click.echo(f"  F2 score (beta=2):          {report.f2_score:.4f}")  # type: ignore[attr-defined]
    click.echo()
    click.echo(f"Path Recall (declaration-scoped k={report.k} grams)")  # type: ignore[attr-defined]
    click.echo("-" * 52)
    path_recall = report.declaration_path_recall_kgram  # type: ignore[attr-defined]
    click.echo(f"  Decl path recall:           {path_recall:.4f}")
    uncovered = report.uncovered_declarations  # type: ignore[attr-defined]
    if uncovered:
        click.echo()
        threshold_pct = f"{report.coverage_threshold:.0%}"  # type: ignore[attr-defined]
        click.echo(f"Uncovered PUT declarations (below {threshold_pct})")
        click.echo("-" * 52)
        for loc, score in uncovered:
            click.echo(f"  {loc}  (best score: {score:.3f})")


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


@cli.command()
def mcp() -> None:
    """Run the Topos MCP server."""
    from topos.server import main as mcp_main

    mcp_main()


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


def _collect_files(
    paths: tuple[str, ...], recursive: bool, language: str
) -> list[Path]:
    """Collect source files for *language* from *paths* (files or directories)."""
    suffixes = language_file_suffixes(language)
    files: set[Path] = set()

    for path_str in paths:
        path = Path(path_str)

        if path.is_file():
            if path.suffix in suffixes:
                files.add(path)
        elif path.is_dir():
            for suffix in suffixes:
                pattern = f"**/*{suffix}" if recursive else f"*{suffix}"
                files.update(path.glob(pattern))

    return sorted(files)


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
    priority: Priority = Priority.BALANCED,
    *,
    language: str = "python",
) -> dict:
    """Evaluate a single file and return results as a dict."""
    try:
        morphism = ProgramMorphism.from_file(filepath, language=language)
        representations = _build_representations(str(filepath), gitnexus_dir)
        result = classifier.classify_detailed(
            morphism, representations=representations, priority=priority
        )

        entropy = result.raw_metrics.get("ast.entropy", 0.0)

        return {
            "file": str(filepath),
            "is_parseable": result.is_parseable,
            "lattice_element": result.summary().name,
            "lattice_symbol": result.summary().symbol,
            "dimensions": {dim: val.name for dim, val in result.dimensions.items()},
            "dimension_symbols": {
                dim: val.symbol for dim, val in result.dimensions.items()
            },
            "scores": {dim: round(s * 100.0, 1) for dim, s in result.scores.items()},
            "priority": priority.value,
            "raw_metrics": result.raw_metrics,
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
            "lattice_element": "BROKEN",
            "lattice_symbol": "⊥",
            "dimensions": {},
            "dimension_symbols": {},
            "scores": {},
            "priority": priority.value,
            "raw_metrics": {},
            "entropy": 0.0,
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
            score = result["scores"].get(dim)
            score_str = f"  [{score:.0f}%]" if score is not None else ""
            click.echo(f"  {dim}: {sym} {name}{score_str}")
        if not result["dimensions"]:
            click.echo("  ⊥ BROKEN (parse failure)")

        if verbose:
            for k, v in result["raw_metrics"].items():
                click.echo(f"    {k}: {v:.3f}")
            if "error" in result:
                click.echo(f"    Error: {result['error']}")


def _output_json(results: list[dict]) -> None:
    """Output results as JSON."""
    import json

    # Strip internal _result key before serialising
    serialisable = [{k: v for k, v in r.items() if k != "_result"} for r in results]
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


if __name__ == "__main__":
    main()
