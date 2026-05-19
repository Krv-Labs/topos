from __future__ import annotations

import sys
from pathlib import Path

import click

from topos.cli.commands.coverage import structural_test_coverage_cmd
from topos.cli.evaluation import (
    collect_files,
    output_json,
    output_overall,
    output_text,
    result_to_row,
    run_classify_file,
)
from topos.core.morphism import ProgramMorphism
from topos.evaluation.characteristic_morphism import CharacteristicMorphism
from topos.evaluation.policies import Priority
from topos.evaluation.policies.simple import describe_entropy_ratio
from topos.functors.probes.ast.entropy import calculate_kolmogorov_proxy
from topos.functors.profunctors.ast.compare import calculate_ast_distance
from topos.graphs.ast.dispatch import SUPPORTED_LANGUAGES, language_file_suffixes

_EVALUATE_LANGUAGE_CHOICE = click.Choice(sorted(SUPPORTED_LANGUAGES))
_PRIORITY_CHOICE = click.Choice([p.value for p in Priority])
_PRIORITY_HELP = (
    "Which quality generator to emphasize in result metadata "
    "(simple / composable / secure). Pass/fail uses fixed per-metric "
    "thresholds in each policy; this flag does not change achieved flags. "
    "For a full generator ranking and relaxation walk, use MCP evaluate "
    "tools with preferences."
)


@click.command()
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
    "output_json_flag",
    is_flag=True,
    help="Output results as JSON.",
)
@click.option(
    "--priority",
    type=_PRIORITY_CHOICE,
    default=Priority.SECURE.value,
    show_default=True,
    help=_PRIORITY_HELP,
)
@click.option(
    "--preferences",
    help=(
        "A comma-separated ranking of quality pillars "
        "(e.g., 'simple,composable,secure')."
    ),
)
@click.option(
    "--gitnexus-dir",
    type=click.Path(exists=True, file_okay=False),
    default=None,
    help=(
        "Path to a .gitnexus/ directory for module dependency metrics. "
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
    output_json_flag: bool,
    priority: str,
    preferences: str | None,
    gitnexus_dir: str | None,
    language: str,
) -> None:
    """Evaluate code quality using the characteristic morphism χ_S : P → Ω."""
    if not paths:
        click.echo("Error: No paths provided.", err=True)
        sys.exit(1)

    # Use the first preference as the priority for CLI output
    if preferences:
        priority = preferences.split(",")[0].strip().lower()
        if priority not in [p.value for p in Priority]:
            click.echo(f"Error: Invalid preference '{priority}'", err=True)
            sys.exit(1)

    files = collect_files(paths, recursive, language)
    if not files:
        suffixes = ", ".join(language_file_suffixes(language))
        click.echo(
            f"No {language} source files found (expected suffixes: {suffixes}).",
            err=True,
        )
        sys.exit(1)

    classifier = CharacteristicMorphism()
    results: list[dict[str, object]] = []
    progress_stream = click.get_text_stream("stderr")
    show_progress = not output_json_flag and len(files) > 1 and progress_stream.isatty()

    if show_progress:
        click.echo(file=progress_stream)
    with click.progressbar(
        files,
        label="Evaluating",
        file=progress_stream,
        hidden=not show_progress,
        show_percent=False,
        show_pos=True,
        show_eta=False,
        fill_char="█",
        empty_char="░",
        width=24,
        bar_template="%(label)s  %(bar)s  %(info)s",
    ) as progress_files:
        try:
            for filepath in progress_files:
                try:
                    cr = run_classify_file(
                        filepath,
                        priority=priority,
                        gitnexus_dir=gitnexus_dir,
                    )
                except (OSError, ValueError) as exc:
                    click.echo(f"Error: {exc}", err=True)
                    sys.exit(1)
                results.append(result_to_row(filepath, cr))
        except KeyboardInterrupt:
            click.echo("Interrupted. Exiting.", err=True)
            sys.exit(130)
    if show_progress:
        click.echo(file=progress_stream)

    if output_json_flag:
        output_json(results)
    else:
        output_text(results, verbose)

    classification_results = [r["_result"] for r in results]
    overall = classifier.combine_dimensions(classification_results)
    output_overall(overall)


@click.command()
@click.argument("source", type=click.Path(exists=True))
@click.argument("target", type=click.Path(exists=True))
@click.option(
    "-v",
    "--verbose",
    is_flag=True,
    help="Show detailed distance metrics.",
)
def compare(source: str, target: str, verbose: bool) -> None:
    """Compare structural distance between two programs."""
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


@click.command()
@click.argument("path", type=click.Path(exists=True))
@click.option(
    "--gitnexus-dir",
    type=click.Path(exists=True, file_okay=False),
    default=None,
    help=(
        "Path to a .gitnexus/ directory for module dependency metrics. "
        "Requires GitNexus (npm install -g gitnexus) — run "
        "'gitnexus analyze' in the repo root to generate this directory."
    ),
)
@click.option(
    "--priority",
    type=_PRIORITY_CHOICE,
    default=Priority.SECURE.value,
    show_default=True,
    help=_PRIORITY_HELP,
)
@click.option(
    "--preferences",
    help=(
        "A comma-separated ranking of quality pillars "
        "(e.g., 'simple,composable,secure')."
    ),
)
def inspect(
    path: str, gitnexus_dir: str | None, priority: str, preferences: str | None
) -> None:
    """Inspect detailed metrics for a single file."""
    # Use the first preference as the priority for CLI output
    if preferences:
        priority = preferences.split(",")[0].strip().lower()
        if priority not in [p.value for p in Priority]:
            click.echo(f"Error: Invalid preference '{priority}'", err=True)
            sys.exit(1)

    morphism = ProgramMorphism.from_file(path)
    result = run_classify_file(Path(path), priority=priority, gitnexus_dir=gitnexus_dir)

    click.echo(f"File: {path}")
    click.echo()

    click.echo("Classification")
    click.echo("-" * 40)
    if not result.is_parseable:
        click.echo("⊥ SLOP — parse failure")
        sys.exit(1)

    for dim, val in result.dimensions.items():
        click.echo(f"  {dim}: {val}")
    click.echo(f"  Valid Syntax: {result.is_parseable}")
    click.echo()

    click.echo("Raw Metrics")
    click.echo("-" * 40)
    for key, value in result.raw_metrics.items():
        interp = result.interpretation.get(key, "")
        suffix = f"  ({interp})" if interp else ""
        click.echo(f"  {key}: {value:.3f}{suffix}")

    if morphism.ast:
        pass

    click.echo()
    click.echo("Entropy Analysis")
    click.echo("-" * 40)
    ratio = calculate_kolmogorov_proxy(morphism.source)
    interp = describe_entropy_ratio(ratio)
    click.echo(f"  Compression Ratio: {ratio:.3f}")
    click.echo(f"  Interpretation: {interp}")


def register_quality_commands(cli_group: click.Group) -> None:
    """Attach quality-analysis commands to the root CLI group."""
    cli_group.add_command(evaluate)
    cli_group.add_command(compare)
    cli_group.add_command(structural_test_coverage_cmd)
    cli_group.add_command(inspect)
