from __future__ import annotations

import json
import sys
from dataclasses import asdict
from pathlib import Path

import click

from topos.graphs.ast.dispatch import SUPPORTED_LANGUAGES, parse_source

_EVALUATE_LANGUAGE_CHOICE = click.Choice(sorted(SUPPORTED_LANGUAGES))


@click.command("structural-test-coverage")
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
    output_json_flag: bool,
    use_v2: bool,
    coverage_threshold: float,
) -> None:
    """Structural overlap of tests toward the program-under-test (UAST)."""
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
        from topos.functors.profunctors.uast.structural_test_coverage import (
            declaration_coverage,
        )
        from topos.evaluation.policies.coverage import score_declaration_coverage

        report_v2 = declaration_coverage(
            put_roots,
            test_roots,
            k=kgram_length,
            include_unknown=include_unknown,
        )
        decision = score_declaration_coverage(report_v2, threshold=coverage_threshold)

        if output_json_flag:
            payload = asdict(report_v2)
            payload.update(asdict(decision))
            payload["language"] = language
            payload["put_paths"] = list(put_paths)
            payload["test_paths"] = list(test_paths)
            click.echo(json.dumps(payload, indent=2))
            return
        _print_v2_report(report_v2, decision, put_paths, test_paths, language)
        return

    from topos.functors.profunctors.uast.structural_test_coverage import (
        structural_test_coverage,
    )

    report = structural_test_coverage(
        put_roots,
        test_roots,
        k=kgram_length,
        include_unknown=include_unknown,
    )

    if output_json_flag:
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
    decision: object,
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
    click.echo(f"  Declaration coverage rate:  {decision.coverage_rate:.4f}")  # type: ignore[attr-defined]
    click.echo(f"  Coverage threshold:         {decision.threshold:.2f}")  # type: ignore[attr-defined]
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
    click.echo(f"  F2 score (beta=2):          {decision.f2_score:.4f}")  # type: ignore[attr-defined]
    click.echo()
    click.echo(f"Path Recall (declaration-scoped k={report.k} grams)")  # type: ignore[attr-defined]
    click.echo("-" * 52)
    path_recall = report.declaration_path_recall_kgram  # type: ignore[attr-defined]
    click.echo(f"  Decl path recall:           {path_recall:.4f}")
    uncovered = decision.uncovered_declarations  # type: ignore[attr-defined]
    if uncovered:
        click.echo()
        threshold_pct = f"{decision.threshold:.0%}"  # type: ignore[attr-defined]
        click.echo(f"Uncovered PUT declarations (below {threshold_pct})")
        click.echo("-" * 52)
        for loc, score in uncovered:
            click.echo(f"  {loc}  (best score: {score:.3f})")
