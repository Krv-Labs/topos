from __future__ import annotations

import json
import sys
from dataclasses import asdict
from pathlib import Path

import click

from topos.graphs.ast.languages import SUPPORTED_LANGUAGES

_EVALUATE_LANGUAGE_CHOICE = click.Choice(sorted(SUPPORTED_LANGUAGES))


@click.command("coverage")
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
    "output_json_flag",
    is_flag=True,
    help="Emit a single JSON object with scores and diagnostics.",
)
@click.option(
    "--coverage-threshold",
    default=0.5,
    show_default=True,
    type=float,
    help="Minimum threshold for coverage policies to pass.",
)
@click.argument(
    "put_paths",
    nargs=-1,
    type=click.Path(exists=True, dir_okay=False),
    required=True,
)
def coverage_cmd(
    put_paths: tuple[str, ...],
    test_paths: tuple[str, ...],
    language: str,
    kgram_length: int,
    include_unknown: bool,
    output_json_flag: bool,
    coverage_threshold: float,
) -> None:
    """Measure structural (UAST) test coverage."""
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

    # UAST declaration bipartite coverage
    from topos.evaluation.policies.coverage import score_declaration_coverage
    from topos.functors.profunctors.uast.structural_test_coverage import (
        declaration_coverage,
    )

    report = declaration_coverage(
        put_roots,
        test_roots,
        k=kgram_length,
        include_unknown=include_unknown,
    )
    decision = score_declaration_coverage(report, threshold=coverage_threshold)

    # Actionable next steps — ranked test targets + a coverage goal.
    suggested_targets = _suggested_test_targets(decision)
    coverage_goal = _coverage_goal(report, decision)

    if output_json_flag:
        payload = asdict(report)
        payload.update(asdict(decision))
        payload["coverage_goal"] = coverage_goal
        payload["suggested_test_targets"] = suggested_targets
        payload["language"] = language
        payload["put_paths"] = list(put_paths)
        payload["test_paths"] = list(test_paths)
        click.echo(json.dumps(payload, indent=2))
        return

    # Human-readable CLI formatting
    click.echo("Topos Structural Test Coverage")
    click.echo(f"Language: {language}")
    click.echo(f"PUT files ({len(put_paths)}): {', '.join(put_paths)}")
    click.echo(f"Test files ({len(test_paths)}): {', '.join(test_paths)}")
    click.echo()
    click.echo("UAST Declaration-Level Coverage")
    click.echo("-" * 52)
    click.echo(f"  Mean declaration coverage:  {report.mean_declaration_coverage:.4f}")
    click.echo(f"  Declaration coverage rate:  {decision.coverage_rate:.4f}")
    click.echo(f"  Coverage threshold:         {decision.threshold:.2f}")
    click.echo(f"  PUT declarations:           {report.put_declaration_count}")
    click.echo(f"  Test declarations:          {report.test_declaration_count}")
    click.echo()
    click.echo("Category-Stratified Recall (disjoint)")
    click.echo("-" * 52)
    click.echo(f"  Statement recall:           {report.stmt_recall:.4f}")
    click.echo(f"  Expression recall:          {report.expr_recall:.4f}")
    click.echo()
    click.echo("Precision and F-score")
    click.echo("-" * 52)
    click.echo(f"  Mean test precision:        {report.mean_test_precision:.4f}")
    click.echo(f"  F2 score (beta=2):          {decision.f2_score:.4f}")
    click.echo()
    click.echo(f"Path Recall (declaration-scoped k={report.k} grams)")
    click.echo("-" * 52)
    click.echo(
        f"  Decl path recall:           {report.declaration_path_recall_kgram:.4f}"
    )

    uncovered = decision.uncovered_declarations
    if uncovered:
        click.echo()
        click.echo(f"Uncovered PUT declarations (below {decision.threshold:.0%})")
        click.echo("-" * 52)
        for loc, score in uncovered:
            click.echo(f"  {loc}  (best score: {score:.3f})")

    # Actionable next steps.
    click.echo()
    click.echo("Suggested Next Steps")
    click.echo("-" * 52)
    click.echo(f"  Goal: {coverage_goal}")
    if suggested_targets:
        click.echo()
        click.echo("  Suggested test targets (write tests for these first):")
        for target in suggested_targets[:10]:
            score = target["best_score"]
            score_note = f" (best score: {score:.3f})" if score is not None else ""
            click.echo(f"    - Add a test exercising {target['target']}{score_note}")
        if len(suggested_targets) > 10:
            click.echo(f"    … and {len(suggested_targets) - 10} more.")
    else:
        click.echo("  All measured declarations and functions meet the threshold.")


def _coverage_goal(report: object, decision: object) -> str:
    """Translate the coverage gap into a concrete, imperative goal."""
    mean = report.mean_declaration_coverage  # type: ignore[attr-defined]
    threshold = decision.threshold  # type: ignore[attr-defined]
    uncovered = decision.uncovered_declarations  # type: ignore[attr-defined]
    if decision.achieved:  # type: ignore[attr-defined]
        return (
            f"Mean coverage {mean:.2f} meets the {threshold:.2f} threshold. "
            "Add tests for any remaining untested functions to harden coverage."
        )
    return (
        f"Mean coverage {mean:.2f} < threshold {threshold:.2f} — add tests for "
        f"{len(uncovered)} uncovered declaration(s) to pass."
    )


def _suggested_test_targets(decision: object) -> list[dict[str, object]]:
    """Rank test targets worst-first: lowest-recall uncovered declarations first."""
    targets: list[dict[str, object]] = []
    uncovered = sorted(
        decision.uncovered_declarations,  # type: ignore[attr-defined]
        key=lambda pair: pair[1],
    )
    for loc, score in uncovered:
        targets.append(
            {"target": loc, "kind": "declaration", "best_score": round(score, 3)}
        )
    return targets
