"""``topos refactor`` — unified refactoring-guidance suite.

Three subcommands sharing one output shape (ranked hotspot table + suggested
action), each surfacing a different structural-analysis engine from the
Methods Upgrade milestone:

- ``cycles``: cycle-basis extraction on the CFG (issue #83).
- ``dependencies``: balanced Forman curvature on the MDG (issue #84).
- ``process``: directed Forman-Ricci curvature on GitNexus process
  graphs (issue #86).

All three are advisory only — none of this affects ``topos evaluate``.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import TYPE_CHECKING

import click

if TYPE_CHECKING:
    from topos.mcp.schemas import RefactorHotspot


@click.group()
def refactor() -> None:
    """Actionable refactor suggestions: cycles, dependencies, process choke points."""


def _print_hotspots(
    title: str, hotspots: list[RefactorHotspot], output_json: bool
) -> None:
    if output_json:
        click.echo(json.dumps([h.model_dump() for h in hotspots], indent=2))
        return

    click.echo(title)
    if not hotspots:
        click.echo("  (none found)")
        return
    for h in hotspots:
        location = h.filepath
        if h.line_start is not None:
            location += f":{h.line_start}"
            if h.line_end is not None and h.line_end != h.line_start:
                location += f"-{h.line_end}"
        click.echo(f"  [{h.kind}] {h.label}  ({location})  score={h.score:.3f}")
        click.echo(f"    -> {h.suggestion}")


@refactor.command("cycles")
@click.argument("path", type=click.Path(exists=True, dir_okay=False))
@click.option(
    "--json", "output_json_flag", is_flag=True, help="Output results as JSON."
)
@click.option(
    "--max-cycles",
    default=5,
    show_default=True,
    type=int,
    help="Maximum cycle hotspots to display.",
)
def cycles_cmd(path: str, output_json_flag: bool, max_cycles: int) -> None:
    """List CFG cycle generators mapped to source line ranges (issue #83)."""
    from topos.core.morphism import ProgramMorphism
    from topos.functors.probes.cfg.homology import calculate_cycle_basis
    from topos.mcp.schemas import RefactorHotspot

    morphism = ProgramMorphism.from_file(path)
    cfg = morphism.build_cfg()
    if cfg is None:
        click.echo(f"Error: could not build a control-flow graph for {path}", err=True)
        raise SystemExit(1)

    result = calculate_cycle_basis(cfg)

    def _span(cycle) -> int:
        if cycle.start_line is None or cycle.end_line is None:
            return 0
        return cycle.end_line - cycle.start_line

    ranked = sorted(result.cycles, key=_span, reverse=True)[:max_cycles]
    hotspots = [
        RefactorHotspot(
            kind="cycle",
            label=f"cycle over blocks {cycle.block_ids}",
            filepath=path,
            line_start=cycle.start_line,
            line_end=cycle.end_line,
            score=float(_span(cycle)),
            suggestion=(
                "Extract this loop/branch body into its own function to "
                "isolate the cycle and shrink cyclomatic complexity."
            ),
        )
        for cycle in ranked
    ]
    title = f"Cycle hotspots (betti_1={result.betti_1}):"
    _print_hotspots(title, hotspots, output_json_flag)


@refactor.command("dependencies")
@click.argument("path", type=click.Path(exists=True, dir_okay=False))
@click.option(
    "--gitnexus-dir",
    type=click.Path(exists=True, file_okay=False),
    default=None,
    help="Path to a .gitnexus/ directory (auto-detected from cwd when omitted).",
)
@click.option(
    "--json", "output_json_flag", is_flag=True, help="Output results as JSON."
)
@click.option(
    "--max-targets",
    default=5,
    show_default=True,
    type=int,
    help="Maximum dependency hotspots to display.",
)
def dependencies_cmd(
    path: str, gitnexus_dir: str | None, output_json_flag: bool, max_targets: int
) -> None:
    """List dependency edges worth strengthening via balanced Forman curvature (#84)."""
    from topos.functors.probes.mdg.curvature import calculate_mdg_curvature
    from topos.mcp.evaluation import load_dep_graph, resolve_gitnexus_dir
    from topos.mcp.schemas import RefactorHotspot

    # An explicit --gitnexus-dir is trusted as-is (already validated by
    # click.Path(exists=True)); auto-detection (no override) falls back to
    # resolve_gitnexus_dir's "<cwd>/.gitnexus if it exists" convention.
    resolved_gitnexus_dir = (
        Path(gitnexus_dir).expanduser().resolve()
        if gitnexus_dir
        else resolve_gitnexus_dir(None, Path.cwd())
    )
    mdg = load_dep_graph(resolved_gitnexus_dir, path)
    if mdg is None:
        click.echo(
            "No .gitnexus dependency graph found. Run 'topos depgraph generate' "
            "first, or pass --gitnexus-dir.",
            err=True,
        )
        raise SystemExit(1)

    file_id = mdg.file_node_id()
    if file_id is None:
        click.echo(f"'{path}' was not found in the dependency graph.", err=True)
        raise SystemExit(1)

    curvature = calculate_mdg_curvature(mdg, file_id)
    ranked = curvature.edges[:max_targets]
    hotspots = [
        RefactorHotspot(
            kind="dependency_edge",
            label=f"{source} -> {target}",
            filepath=path,
            score=score,
            suggestion=(
                "Highly negative curvature: many otherwise-unrelated modules "
                "route coupling through this edge. Consider extracting a "
                "shared interface to strengthen it."
                if score < 0
                else "Well-supported dependency edge; no action needed."
            ),
        )
        for source, target, score in ranked
    ]
    _print_hotspots("Dependency hotspots:", hotspots, output_json_flag)


@refactor.command("process")
@click.argument("path", type=click.Path(exists=True, dir_okay=False))
@click.option(
    "--gitnexus-dir",
    type=click.Path(exists=True, file_okay=False),
    default=None,
    help="Path to a .gitnexus/ directory (auto-detected from cwd when omitted).",
)
@click.option(
    "--json", "output_json_flag", is_flag=True, help="Output results as JSON."
)
@click.option(
    "--max-targets",
    default=5,
    show_default=True,
    type=int,
    help="Maximum process hotspots to display.",
)
def process_cmd(
    path: str, gitnexus_dir: str | None, output_json_flag: bool, max_targets: int
) -> None:
    """List process-graph choke points via directed Forman-Ricci curvature (#86)."""
    from topos.functors.probes.process.curvature import calculate_process_curvature
    from topos.graphs.process.object import ProcessGraph
    from topos.mcp.evaluation import load_dep_graph, resolve_gitnexus_dir
    from topos.mcp.schemas import RefactorHotspot

    # An explicit --gitnexus-dir is trusted as-is (already validated by
    # click.Path(exists=True)); auto-detection (no override) falls back to
    # resolve_gitnexus_dir's "<cwd>/.gitnexus if it exists" convention.
    resolved_gitnexus_dir = (
        Path(gitnexus_dir).expanduser().resolve()
        if gitnexus_dir
        else resolve_gitnexus_dir(None, Path.cwd())
    )
    mdg = load_dep_graph(resolved_gitnexus_dir, path)
    if mdg is None:
        click.echo(
            "No .gitnexus dependency graph found. Run 'topos depgraph generate' "
            "first, or pass --gitnexus-dir.",
            err=True,
        )
        raise SystemExit(1)

    file_id = mdg.file_node_id()
    if file_id is None:
        click.echo(f"'{path}' was not found in the dependency graph.", err=True)
        raise SystemExit(1)

    process_graph = ProcessGraph.from_mdg(mdg, path)
    touching = process_graph.paths_touching_file(file_id)
    subgraph = ProcessGraph(target_file=path, paths=touching)
    curvature = calculate_process_curvature(subgraph)

    ranked = curvature.edges[:max_targets]
    hotspots = [
        RefactorHotspot(
            kind="process_transition",
            label=f"{source} -> {target}",
            filepath=path,
            score=score,
            suggestion=(
                "Choke point: many independent execution paths squeeze "
                "through this transition. Consider an asynchronous "
                "decoupling boundary (message queue / pub-sub)."
                if score < 0
                else "Well-distributed transition; no action needed."
            ),
        )
        for source, target, score in ranked
    ]
    _print_hotspots("Process choke points:", hotspots, output_json_flag)


def register_refactor_commands(cli_group: click.Group) -> None:
    """Attach the refactoring suite to the root CLI group."""
    cli_group.add_command(refactor)
