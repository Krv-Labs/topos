"""Unified refactoring-guidance suite (Methods Upgrade milestone).

One tool, three targets (ranked hotspot list + suggested action), each
surfacing a different structural-analysis engine:

- ``target="cycles"`` (issue #83): cycle-basis extraction on the CFG,
  pointing cyclomatic complexity's count at the actual source loops.
- ``target="dependencies"`` (issue #84): balanced Forman curvature on
  the MDG, naming concrete dependency edges worth strengthening.
- ``target="process"`` (issue #86): directed Forman-Ricci curvature on
  GitNexus process graphs, finding execution "choke points".

All three are purely advisory — none of this feeds SIMPLE/COMPOSABLE/SECURE
scoring; that's an explicit acceptance criterion carried over from #86 and
applied consistently across the whole suite. Orientation:
``topos_get_doc(topic="workflows")`` (Advisory refactoring) and
``openwiki/workflows/agent-and-cli.md`` (repo filesystem; not an MCP resource).
"""

from __future__ import annotations

from fastmcp.tools.base import ToolResult

from topos.core.morphism import ProgramMorphism
from topos.functors.probes.cfg.homology import calculate_cycle_basis
from topos.functors.probes.mdg.curvature import calculate_mdg_curvature
from topos.functors.probes.process.curvature import calculate_process_curvature
from topos.graphs.process.object import ProcessGraph

from ..evaluation import detect_language, load_dep_graph, resolve_gitnexus_dir
from ..formatting import to_tool_result
from ..refactor_hotspots import render_hotspots_md
from ..schemas import RefactorHotspot, RefactorInput, RefactorResult
from ..security import read_safe_utf8_file, resolve_file_root, resolve_within_root
from ..server import mcp

_READ_ONLY_ANN = {
    "title": "Topos Refactor Suggestions",
    "readOnlyHint": True,
    "destructiveHint": False,
    "idempotentHint": True,
    "openWorldHint": False,
}


@mcp.tool(
    name="topos_refactor",
    tags={"refactor", "cfg", "mdg", "process"},
    annotations=_READ_ONLY_ANN,
)
def topos_refactor(params: RefactorInput) -> ToolResult:
    """Refactor hotspots (read-only). See topos_get_doc(topic="workflows")."""
    try:
        if params.target == "cycles":
            return _refactor_cycles(params)
        if params.target == "dependencies":
            return _refactor_dependencies(params)
        return _refactor_process(params)
    except Exception as exc:
        titles = {
            "cycles": "Cycle hotspots",
            "dependencies": "Dependency hotspots",
            "process": "Process choke points",
        }
        model = RefactorResult(
            target=params.target,
            filepath=params.filepath,
            error=str(exc),
        )
        return to_tool_result(model, render_hotspots_md(titles[params.target], []))


def _refactor_cycles(params: RefactorInput) -> ToolResult:
    resolved, err = resolve_within_root(params.filepath)
    if err or resolved is None:
        model = RefactorResult(
            target="cycles",
            filepath=params.filepath,
            error=(err or {}).get("error", "path error"),
        )
        return to_tool_result(model, render_hotspots_md("Cycle hotspots", []))

    source, read_err = read_safe_utf8_file(resolved)
    if read_err:
        model = RefactorResult(
            target="cycles", filepath=params.filepath, error=read_err["error"]
        )
        return to_tool_result(model, render_hotspots_md("Cycle hotspots", []))

    language = detect_language(resolved)
    morphism = ProgramMorphism(source=source, language=language)
    cfg = morphism.build_cfg()
    if cfg is None:
        model = RefactorResult(
            target="cycles",
            filepath=params.filepath,
            error="Could not build a control-flow graph.",
        )
        return to_tool_result(model, render_hotspots_md("Cycle hotspots", []))

    result = calculate_cycle_basis(cfg)

    def _span(cycle) -> int:
        if cycle.start_line is None or cycle.end_line is None:
            return 0
        return cycle.end_line - cycle.start_line

    ranked = sorted(result.cycles, key=_span, reverse=True)[: params.limit]
    hotspots = [
        RefactorHotspot(
            kind="cycle",
            label=f"cycle over blocks {cycle.block_ids}",
            filepath=params.filepath,
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

    model = RefactorResult(
        target="cycles",
        filepath=params.filepath,
        betti_1=result.betti_1,
        hotspots=hotspots,
    )
    title = f"Cycle hotspots (betti_1={result.betti_1})"
    return to_tool_result(model, render_hotspots_md(title, hotspots))


def _refactor_dependencies(params: RefactorInput) -> ToolResult:
    project_root = resolve_file_root()
    gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir, project_root)
    mdg = load_dep_graph(gitnexus_dir, params.filepath)
    if mdg is None:
        model = RefactorResult(
            target="dependencies", filepath=params.filepath, gitnexus_available=False
        )
        return to_tool_result(model, render_hotspots_md("Dependency hotspots", []))

    file_id = mdg.file_node_id()
    if file_id is None:
        model = RefactorResult(
            target="dependencies",
            filepath=params.filepath,
            gitnexus_available=True,
            hotspots=[],
        )
        return to_tool_result(model, render_hotspots_md("Dependency hotspots", []))

    curvature = calculate_mdg_curvature(mdg, file_id)
    ranked = curvature.edges[: params.limit]
    hotspots = [
        RefactorHotspot(
            kind="dependency_edge",
            label=f"{src} -> {dst}",
            filepath=params.filepath,
            score=score,
            suggestion=(
                "Highly negative curvature: many otherwise-unrelated modules "
                "route coupling through this edge. Consider extracting a "
                "shared interface or reducing the shared surface to "
                "strengthen it."
                if score < 0
                else "Well-supported dependency edge; no action needed."
            ),
        )
        for src, dst, score in ranked
    ]

    model = RefactorResult(
        target="dependencies",
        filepath=params.filepath,
        gitnexus_available=True,
        hotspots=hotspots,
    )
    return to_tool_result(model, render_hotspots_md("Dependency hotspots", hotspots))


def _refactor_process(params: RefactorInput) -> ToolResult:
    project_root = resolve_file_root()
    gitnexus_dir = resolve_gitnexus_dir(params.gitnexus_dir, project_root)
    mdg = load_dep_graph(gitnexus_dir, params.filepath)
    if mdg is None:
        model = RefactorResult(
            target="process", filepath=params.filepath, gitnexus_available=False
        )
        return to_tool_result(model, render_hotspots_md("Process choke points", []))

    file_id = mdg.file_node_id()
    if file_id is None:
        model = RefactorResult(
            target="process",
            filepath=params.filepath,
            gitnexus_available=True,
            hotspots=[],
        )
        return to_tool_result(model, render_hotspots_md("Process choke points", []))

    process_graph = ProcessGraph.from_mdg(mdg, params.filepath)
    touching = process_graph.paths_touching_file(file_id)
    subgraph = ProcessGraph(target_file=params.filepath, paths=touching)
    curvature = calculate_process_curvature(subgraph)

    ranked = curvature.edges[: params.limit]
    hotspots = [
        RefactorHotspot(
            kind="process_transition",
            label=f"{src} -> {dst}",
            filepath=params.filepath,
            score=score,
            suggestion=(
                "Choke point: many independent execution paths squeeze "
                "through this transition. Consider an asynchronous "
                "decoupling boundary (message queue / pub-sub), or "
                "acknowledge the simplicity trade-off of keeping it."
                if score < 0
                else "Well-distributed transition; no action needed."
            ),
        )
        for src, dst, score in ranked
    ]

    model = RefactorResult(
        target="process",
        filepath=params.filepath,
        gitnexus_available=True,
        hotspots=hotspots,
    )
    return to_tool_result(model, render_hotspots_md("Process choke points", hotspots))
