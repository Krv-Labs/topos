"""
Process Graph Representation
=============================
Consumes the ``Process`` nodes and ``STEP_IN_PROCESS`` relationships already
present in the GitNexus knowledge graph (see
:mod:`topos.graphs.mdg.object`) and lifts them into ordered execution paths.

This is a **refactoring-tool** input, not a scored
:class:`~topos.graphs.base.Representation`: process-graph analysis
(issue #86) must never influence the SIMPLE, COMPOSABLE, or SECURE medal
computation. It exists purely to feed the directed
Forman-Ricci curvature engine (:mod:`topos.functors.probes.process.curvature`)
that powers ``topos refactor process``.

Reuses :class:`~topos.graphs.mdg.object.ModuleDependencyGraph`'s existing
ladybug-loading machinery (including schema-mismatch handling) rather than
opening a second connection to ``.gitnexus/lbug`` — a :class:`ProcessGraph`
is built by loading a full MDG, then filtering it down to ``Process`` /
``STEP_IN_PROCESS`` structure.
"""

from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from topos.graphs.mdg.object import ModuleDependencyGraph


@dataclass
class ProcessStep:
    """One step (code element) participating in a process execution path."""

    node_id: str
    label: str
    step: int
    properties: dict[str, object] = field(default_factory=dict)


@dataclass
class ProcessPath:
    """An ordered execution path through a single ``Process`` node."""

    process_id: str
    steps: list[ProcessStep] = field(default_factory=list)


@dataclass
class ProcessGraph:
    """
    Ordered execution-path view of GitNexus ``Process`` / ``STEP_IN_PROCESS``
    data, scoped to a target file for the ``topos refactor process`` tool.

    Attributes:
        target_file: The file path this graph was built to analyze.
        paths: Every ``Process`` node's execution path, steps sorted ascending.
    """

    target_file: str
    paths: list[ProcessPath] = field(default_factory=list)

    _mdg: ModuleDependencyGraph | None = field(default=None, repr=False, compare=False)

    @classmethod
    def from_mdg(cls, mdg: ModuleDependencyGraph, target_file: str) -> ProcessGraph:
        """Filter an already-loaded MDG down to Process/STEP_IN_PROCESS structure."""
        graph = cls(target_file=target_file, _mdg=mdg)
        process_ids = {n.id for n in mdg.nodes_of_label("Process")}

        rels_by_process: dict[str, list] = defaultdict(list)
        for rel in mdg.relationships_of_type("STEP_IN_PROCESS"):
            if rel.source_id in process_ids:
                rels_by_process[rel.source_id].append(rel)

        for process_id, rels in rels_by_process.items():
            steps = []
            for fallback_order, rel in enumerate(rels):
                target_node = mdg.get_node(rel.target_id)
                label = target_node.label if target_node is not None else ""
                step_value = rel.properties.get("step")
                step_index = (
                    int(step_value) if step_value is not None else fallback_order
                )
                steps.append(
                    ProcessStep(
                        node_id=rel.target_id,
                        label=label,
                        step=step_index,
                        properties=dict(rel.properties),
                    )
                )
            steps.sort(key=lambda s: s.step)
            graph.paths.append(ProcessPath(process_id=process_id, steps=steps))

        return graph

    @classmethod
    def from_gitnexus_dir(
        cls, gitnexus_dir: str | Path, target_file: str
    ) -> ProcessGraph:
        """Load a full MDG from ``.gitnexus/`` and filter it to process structure."""
        from topos.graphs.mdg.object import ModuleDependencyGraph

        mdg = ModuleDependencyGraph.from_gitnexus_dir(gitnexus_dir, target_file)
        return cls.from_mdg(mdg, target_file)

    def paths_touching_file(self, file_node_id: str) -> list[ProcessPath]:
        """Paths where any step's underlying symbol is contained in `file_node_id`."""
        if self._mdg is None:
            return []
        symbol_ids = set(self._mdg.all_contained_symbols(file_node_id))
        symbol_ids.add(file_node_id)
        return [
            path
            for path in self.paths
            if any(step.node_id in symbol_ids for step in path.steps)
        ]

    def edges(self) -> list[tuple[str, str]]:
        """Flatten every path's consecutive step pairs into directed edges —
        the input to ``frc.directed_forman_curvature``."""
        result: list[tuple[str, str]] = []
        for path in self.paths:
            for a, b in zip(path.steps, path.steps[1:], strict=False):
                result.append((a.node_id, b.node_id))
        return result
