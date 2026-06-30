"""
Process-flow representation (GitNexus dynamic behavior).

GitNexus reconstructs *execution flows* — ordered call sequences ("Processes")
— from the static call graph and stores them in ``.gitnexus/lbug`` as
``Process`` nodes joined to their steps by ``STEP_IN_PROCESS`` edges. This is
the closest signal the index has to dynamic behavior: the interprocedural
sequence a program runs, recovered without execution.

This module lifts those flows into a :class:`~topos.graphs.base.Representation`.
It is the **interprocedural** complement to the intra-procedural CFG/CPG and
the module-level :class:`~topos.graphs.mdg.object.ModuleDependencyGraph`:

    SIMPLE      flow length / participation   (complexity *between* functions)
    COMPOSABLE  community span / crossings    (flow-level coupling)
    SECURE      dangerous-step reachability   (sinks on a real execution flow)

The flows are parsed from an already-loaded ``ModuleDependencyGraph`` (whose
LadybugDB loader already reads every node and edge), so no second DB read is
paid. A single ``Representation`` feeds one pillar; use
:meth:`ProcessFlowGraph.for_dimension` to fan one parsed flow list out across
SIMPLE / COMPOSABLE / SECURE.
"""

from __future__ import annotations

from dataclasses import dataclass, field, replace
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.mdg.object import ModuleDependencyGraph


@dataclass(frozen=True)
class ProcessFlow:
    """One reconstructed execution flow that touches the target file.

    Attributes:
        id:             GitNexus process id (e.g. ``proc_0_curate_composable``).
        label:          Human-readable flow label (``entry -> terminal``).
        process_type:   GitNexus classification (e.g. ``cross_community``).
        step_count:     Number of steps in the flow.
        communities:    Community ids the flow spans.
        entry_point_id: Symbol id of the flow's first step.
        terminal_id:    Symbol id of the flow's last step.
        step_symbol_ids: Symbol ids of every step in the flow.
    """

    id: str
    label: str
    process_type: str
    step_count: int
    communities: tuple[str, ...]
    entry_point_id: str
    terminal_id: str
    step_symbol_ids: tuple[str, ...]

    def step_names(self) -> list[str]:
        """Bare symbol names for every step (and the entry/terminal symbols)."""
        ids = {self.entry_point_id, self.terminal_id, *self.step_symbol_ids}
        return [_name_from_symbol_id(sid) for sid in ids if sid]


@dataclass
class ProcessFlowGraph:
    """Execution flows touching ``target_file``, as a ``Representation``.

    One instance feeds a single pillar (``dimension_axis``). Build once with
    :meth:`from_dep_graph`, then :meth:`for_dimension` to share the parsed flow
    list across the three pillars.
    """

    target_file: str
    flows: tuple[ProcessFlow, ...] = field(default_factory=tuple)
    language: str = "python"
    dimension_axis: str = "simple"

    @property
    def name(self) -> str:
        return "process"

    @property
    def dimension(self) -> str:
        return self.dimension_axis

    # ------------------------------------------------------------------
    # Construction
    # ------------------------------------------------------------------

    @classmethod
    def from_dep_graph(
        cls,
        graph: ModuleDependencyGraph,
        target_file: str,
        *,
        language: str = "python",
        dimension: str = "simple",
    ) -> ProcessFlowGraph:
        """Parse the flows touching ``target_file`` from a loaded MDG."""
        flows: list[ProcessFlow] = []
        for node in graph.nodes_of_label("Process"):
            props = node.properties
            step_ids = tuple(
                rel.source_id for rel in graph.incoming(node.id, "STEP_IN_PROCESS")
            )
            entry = str(props.get("entryPointId", ""))
            terminal = str(props.get("terminalId", ""))
            flow = ProcessFlow(
                id=node.id,
                label=str(props.get("label") or props.get("heuristicLabel") or node.id),
                process_type=str(props.get("processType", "")),
                step_count=_as_int(props.get("stepCount"), default=len(step_ids)),
                communities=_clean_communities(props.get("communities")),
                entry_point_id=entry,
                terminal_id=terminal,
                step_symbol_ids=step_ids,
            )
            if _flow_touches_file(flow, target_file):
                flows.append(flow)

        return cls(
            target_file=target_file,
            flows=tuple(flows),
            language=language,
            dimension_axis=dimension,
        )

    def for_dimension(self, dimension: str) -> ProcessFlowGraph:
        """Return a view of these flows scored on a different pillar."""
        return replace(self, dimension_axis=dimension)

    # ------------------------------------------------------------------
    # Metrics
    # ------------------------------------------------------------------

    def metrics(self) -> dict[str, float]:
        from topos.functors.probes.process.flow import (
            cross_community_flow_count,
            dangerous_flow_count,
            flow_participation,
            max_community_span,
            max_flow_length,
        )

        flows = list(self.flows)
        if self.dimension_axis == "simple":
            return {
                "process.max_flow_length": float(max_flow_length(flows)),
                "process.flow_participation": float(flow_participation(flows)),
            }
        if self.dimension_axis == "composable":
            return {
                "process.max_community_span": float(max_community_span(flows)),
                "process.cross_community_flows": float(
                    cross_community_flow_count(flows)
                ),
            }
        if self.dimension_axis == "secure":
            return {
                "process.dangerous_flows": float(
                    dangerous_flow_count(flows, self.language)
                ),
            }
        return {}


# ---------------------------------------------------------------------------
# Symbol-id helpers
# ---------------------------------------------------------------------------
# GitNexus symbol ids embed the source path: "<Label>:<path>:<name>".


def _path_from_symbol_id(symbol_id: str) -> str:
    parts = symbol_id.split(":")
    if len(parts) >= 3:
        return ":".join(parts[1:-1])
    return ""


def _name_from_symbol_id(symbol_id: str) -> str:
    if not symbol_id:
        return ""
    return symbol_id.rsplit(":", 1)[-1]


def _paths_match(symbol_path: str, target_file: str) -> bool:
    """Suffix-aware path match, mirroring ``ModuleDependencyGraph.file_node_id``."""
    if not symbol_path:
        return False
    return (
        symbol_path == target_file
        or symbol_path.endswith(f"/{target_file}")
        or target_file.endswith(f"/{symbol_path}")
    )


def _flow_touches_file(flow: ProcessFlow, target_file: str) -> bool:
    candidate_ids = (flow.entry_point_id, flow.terminal_id, *flow.step_symbol_ids)
    return any(
        _paths_match(_path_from_symbol_id(sid), target_file) for sid in candidate_ids
    )


def _as_int(value: object, *, default: int) -> int:
    try:
        return int(value)  # type: ignore[arg-type]
    except (TypeError, ValueError):
        return default


def _clean_communities(value: object) -> tuple[str, ...]:
    """Normalize the ``communities`` property into a tuple of bare ids.

    GitNexus stores these as a list whose elements may carry stray quotes
    (e.g. ``"'comm_87'"``); strip them so ids compare cleanly.
    """
    if not isinstance(value, (list, tuple)):
        return ()
    return tuple(str(item).strip().strip("'\"") for item in value if item)
