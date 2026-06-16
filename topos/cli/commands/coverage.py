from __future__ import annotations

import json
import sys
from dataclasses import asdict
from pathlib import Path

import click

from topos.graphs.ast.dispatch import SUPPORTED_LANGUAGES, parse_source

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
    """Measure structural (UAST) and semantic (CPG Topological) test coverage."""
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

    # 1. Existing UAST declaration bipartite coverage
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

    # 2. CPG topological ECT coverage (optional extra)
    from topos.evaluation.policies.coverage import score_topological_coverage
    from topos.functors.profunctors.cpg.topological_coverage import (
        ECT_COVERAGE_INSTALL_HINT,
        ECTCoverageUnavailableError,
        calculate_topological_coverage,
        ect_coverage_available,
    )
    from topos.graphs.cpg.object import CodePropertyGraph

    topo_report = None
    topo_decision = None
    topo_unavailable_reason: str | None = None

    if ect_coverage_available():
        # Build and merge CodePropertyGraphs
        put_cpgs = []
        for path in put_paths:
            source = Path(path).read_text(encoding="utf-8")
            result = parse_source(source=source, language=language, file=str(path))
            if result.uast_root is not None:
                put_cpgs.append(
                    CodePropertyGraph.from_uast(result.uast_root, source=source)
                )

        test_cpgs = []
        for path in test_paths:
            source = Path(path).read_text(encoding="utf-8")
            result = parse_source(source=source, language=language, file=str(path))
            if result.uast_root is not None:
                test_cpgs.append(
                    CodePropertyGraph.from_uast(result.uast_root, source=source)
                )

        def merge_cpgs(cpgs: list[CodePropertyGraph]) -> CodePropertyGraph:
            if not cpgs:
                return CodePropertyGraph()
            merged_nodes = {}
            merged_edges = []
            sources = []
            for c in cpgs:
                merged_nodes.update(c.nodes)
                merged_edges.extend(c.edges)
                if c.source:
                    sources.append(c.source)
            return CodePropertyGraph(
                nodes=merged_nodes,
                edges=merged_edges,
                language=cpgs[0].language,
                source="\n".join(sources),
            )

        put_cpg = merge_cpgs(put_cpgs)
        test_cpg = merge_cpgs(test_cpgs)

        try:
            topo_report = calculate_topological_coverage(put_cpg, test_cpg)
            topo_decision = score_topological_coverage(
                topo_report, threshold=coverage_threshold
            )
        except ECTCoverageUnavailableError as exc:
            topo_unavailable_reason = str(exc)
    else:
        topo_unavailable_reason = (
            "Topological (ECT) coverage requires the optional ect-coverage extra. "
            f"Install with: {ECT_COVERAGE_INSTALL_HINT}"
        )

    if output_json_flag:
        payload = asdict(report)
        payload.update(asdict(decision))
        if topo_report is not None and topo_decision is not None:
            payload["topological_coverage"] = {
                "distance": topo_report.topological_distance,
                "coverage_score": topo_report.topological_coverage_score,
                "tested_functions": list(topo_report.tested_functions),
                "untested_functions": list(topo_report.untested_functions),
                "put_node_count": topo_report.put_node_count,
                "test_node_count": topo_report.test_node_count,
                "scoped_node_count": topo_report.scoped_node_count,
                "achieved": topo_decision.achieved,
                "threshold": topo_decision.threshold,
                "interpretation": topo_decision.interpretation,
            }
        else:
            payload["topological_coverage"] = {
                "unavailable": True,
                "reason": topo_unavailable_reason,
            }
        payload["language"] = language
        payload["put_paths"] = list(put_paths)
        payload["test_paths"] = list(test_paths)
        click.echo(json.dumps(payload, indent=2))
        return

    # Human-readable CLI formatting
    click.echo("Topos Structural & Semantic Test Coverage")
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

    click.echo()
    click.echo("Topological CPG Semantic Coverage")
    click.echo("-" * 52)
    if topo_report is not None and topo_decision is not None:
        score = topo_report.topological_coverage_score
        dist = topo_report.topological_distance
        click.echo(f"  Topological coverage score: {score:.4f}")
        click.echo(f"  Topological ECT distance:   {dist:.4f}")
        click.echo(f"  Topological threshold:      {topo_decision.threshold:.2f}")
        click.echo(f"  Topological target met:     {str(topo_decision.achieved)}")
        click.echo(f"  PUT CPG node count:         {topo_report.put_node_count}")
        click.echo(f"  Test CPG node count:        {topo_report.test_node_count}")
        click.echo(f"  Scoped PUT nodes (reach):   {topo_report.scoped_node_count}")

        if topo_report.tested_functions:
            click.echo()
            click.echo(f"Tested PUT Functions ({len(topo_report.tested_functions)})")
            click.echo("-" * 52)
            for func in topo_report.tested_functions:
                click.echo(f"  {func}")

        if topo_report.untested_functions:
            click.echo()
            click.echo(
                f"Untested PUT Functions ({len(topo_report.untested_functions)})"
            )
            click.echo("-" * 52)
            for func in topo_report.untested_functions:
                click.echo(f"  {func}")
    else:
        click.echo(f"  Unavailable: {topo_unavailable_reason}")
