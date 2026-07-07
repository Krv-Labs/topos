from __future__ import annotations

import sys
from pathlib import Path

import click

from topos.evaluation.policies.base import Priority
from topos.graphs.ast.languages import SUPPORTED_LANGUAGES, language_file_suffixes

_EVALUATE_LANGUAGE_CHOICE = click.Choice(sorted(SUPPORTED_LANGUAGES))
_PRIORITY_CHOICE = click.Choice([p.value for p in Priority])
_PRIORITY_HELP = (
    "Which quality generator to emphasize in result metadata "
    "(simple / composable / secure). Pass/fail uses fixed per-metric "
    "thresholds in each policy; this flag does not change achieved flags. "
    "For a full generator ranking and relaxation walk, use MCP evaluate "
    "tools with preferences."
)
_ALLOW_HELP = (
    "Acknowledge a dangerous-call pattern for this run (repeatable / "
    "comma-separated, e.g. --allow yaml.load). The raw SECURE verdict is "
    "always still computed and shown, and any acknowledgement caps the grade "
    "below Gold/IDEAL. For persistent project rules use a .topos.toml file."
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
    "--allow",
    "allows",
    multiple=True,
    help=_ALLOW_HELP,
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
    allows: tuple[str, ...],
    language: str,
) -> None:
    """Evaluate code quality using the characteristic morphism χ_S : P → Ω."""
    from topos.cli.diagnostics import (
        acknowledged_to_dict,
        collect_findings_and_verdict,
        finding_to_dict,
        suggestion_to_dict,
        suggestions_for,
    )
    from topos.cli.evaluation import (
        collect_files,
        output_directory_average,
        output_json,
        output_overall,
        output_text,
        result_to_row,
        run_classify_file,
    )
    from topos.config import load_topos_config, merge_cli_allows
    from topos.evaluation.characteristic_morphism import CharacteristicMorphism

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

    config = merge_cli_allows(load_topos_config(Path(paths[0])), allows)
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
                row = result_to_row(filepath, cr)
                active, acknowledged, verdict = collect_findings_and_verdict(
                    filepath, cr, config
                )
                suggestions = suggestions_for(cr, active)
                row["security_findings"] = [finding_to_dict(f) for f in active]
                row["acknowledged_risks"] = acknowledged_to_dict(acknowledged)
                row["suggestions"] = [suggestion_to_dict(s) for s in suggestions]
                row["secure_raw"] = verdict.raw_secure_pass
                row["secure_adjusted"] = verdict.adjusted_secure_pass
                row["grade_capped"] = verdict.grade_capped
                row["_active_findings"] = active
                row["_acknowledged"] = acknowledged
                row["_verdict"] = verdict
                row["_suggestions"] = suggestions
                results.append(row)
        except KeyboardInterrupt:
            click.echo("Interrupted. Exiting.", err=True)
            sys.exit(130)
    if show_progress:
        click.echo(file=progress_stream)

    if output_json_flag:
        output_json(results)
        return
    else:
        output_text(results, verbose)

    classification_results = [r["_result"] for r in results]
    overall = classifier.combine_dimensions(classification_results)
    output_directory_average(results)
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
    from topos.core.morphism import ProgramMorphism
    from topos.functors.profunctors.ast.compare import calculate_ast_distance

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
@click.option(
    "--allow",
    "allows",
    multiple=True,
    help=_ALLOW_HELP,
)
@click.option(
    "--json",
    "output_json_flag",
    is_flag=True,
    help="Output the inspection as a single JSON object.",
)
def inspect(
    path: str,
    gitnexus_dir: str | None,
    priority: str,
    preferences: str | None,
    allows: tuple[str, ...],
    output_json_flag: bool,
) -> None:
    """Inspect detailed metrics for a single file."""
    from topos.cli.diagnostics import (
        collect_findings_and_verdict,
        render_security_findings,
        render_suggestions,
        render_verdict_line,
        suggestions_for,
    )
    from topos.cli.evaluation import run_classify_file
    from topos.config import load_topos_config, merge_cli_allows
    from topos.core.morphism import ProgramMorphism
    from topos.evaluation.policies.simple import describe_entropy_ratio
    from topos.functors.probes.ast.entropy import calculate_kolmogorov_proxy
    from topos.mcp.evaluation import detect_language

    # Use the first preference as the priority for CLI output
    if preferences:
        priority = preferences.split(",")[0].strip().lower()
        if priority not in [p.value for p in Priority]:
            click.echo(f"Error: Invalid preference '{priority}'", err=True)
            sys.exit(1)

    morphism = ProgramMorphism.from_file(path, language=detect_language(Path(path)))
    result = run_classify_file(Path(path), priority=priority, gitnexus_dir=gitnexus_dir)

    config = merge_cli_allows(load_topos_config(Path(path)), allows)
    active, acknowledged, verdict = collect_findings_and_verdict(path, result, config)
    suggestions = suggestions_for(result, active)

    if output_json_flag:
        _inspect_json(path, result, active, acknowledged, verdict, suggestions)
        return

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

    render_security_findings(active, acknowledged)
    render_verdict_line(verdict)
    render_suggestions(suggestions)

    click.echo()
    click.echo("Entropy Analysis")
    click.echo("-" * 40)
    ratio = calculate_kolmogorov_proxy(morphism.source)
    interp = describe_entropy_ratio(ratio)
    click.echo(f"  Compression Ratio: {ratio:.3f}")
    click.echo(f"  Interpretation: {interp}")


def _inspect_json(path, result, active, acknowledged, verdict, suggestions) -> None:  # type: ignore[no-untyped-def]
    """Emit the single-file inspection as JSON."""
    import json

    from topos._version import __version__
    from topos.cli.diagnostics import (
        acknowledged_to_dict,
        finding_to_dict,
        suggestion_to_dict,
    )

    payload = {
        "version": __version__,
        "file": str(path),
        "is_parseable": result.is_parseable,
        "lattice_element": result.summary().name,
        "dimensions": {d: v.name for d, v in result.dimensions.items()},
        "scores": {d: round(s * 100.0, 1) for d, s in result.scores.items()},
        "raw_metrics": dict(result.raw_metrics),
        "secure_raw": verdict.raw_secure_pass,
        "secure_adjusted": verdict.adjusted_secure_pass,
        "grade_capped": verdict.grade_capped,
        "adjusted_lattice_element": verdict.adjusted_element.name,
        "security_findings": [finding_to_dict(f) for f in active],
        "acknowledged_risks": acknowledged_to_dict(acknowledged),
        "suggestions": [suggestion_to_dict(s) for s in suggestions],
    }
    click.echo(json.dumps(payload, indent=2))


def register_quality_commands(cli_group: click.Group) -> None:
    """Attach quality-analysis commands to the root CLI group."""
    from topos.cli.commands.coverage import coverage_cmd

    cli_group.add_command(evaluate)
    cli_group.add_command(compare)
    cli_group.add_command(coverage_cmd)
    cli_group.add_command(inspect)
