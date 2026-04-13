"""
Dependency Graph Representation
-------------------------------
Consumes the knowledge graph produced by `GitNexus <https://github.com/abhigyanpatwari/GitNexus>`_
and lifts it into a :class:`~topos.graphs.base.Representation`.

GitNexus runs ``gitnexus analyze`` on a repository and writes a
``.gitnexus/`` directory containing a LadybugDB graph store.  This
module parses that output into an in-memory graph of typed nodes
and relationships, then computes dependency-level metrics that the
AST alone cannot provide.

Node labels and relationship types mirror GitNexus's shared schema::

    Nodes:  File, Module, Function, Class, Method, Import, ...
    Edges:  CALLS, IMPORTS, INHERITS, CONTAINS, USES, ...

Metrics produced:
    - ``depgraph.coupling``   -- afferent + efferent coupling for a file
    - ``depgraph.instability`` -- Ce / (Ca + Ce)
    - ``depgraph.fan_in``      -- incoming CALLS edges
    - ``depgraph.fan_out``     -- outgoing CALLS edges
    - ``depgraph.dep_depth``   -- longest IMPORTS chain from the file
"""

from __future__ import annotations

import json
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Literal

NodeLabel = Literal[
    "Project",
    "Package",
    "Module",
    "Folder",
    "File",
    "Class",
    "Function",
    "Method",
    "Variable",
    "Interface",
    "Enum",
    "Decorator",
    "Import",
    "Type",
    "CodeElement",
    "Community",
    "Process",
    "Struct",
    "Namespace",
    "Trait",
    "Constructor",
]

RelationshipType = Literal[
    "CONTAINS",
    "CALLS",
    "INHERITS",
    "IMPORTS",
    "USES",
    "DEFINES",
    "DECORATES",
    "IMPLEMENTS",
    "EXTENDS",
    "HAS_METHOD",
    "HAS_PROPERTY",
    "ACCESSES",
    "MEMBER_OF",
    "METHOD_OVERRIDES",
    "METHOD_IMPLEMENTS",
    "STEP_IN_PROCESS",
]


@dataclass
class GraphNode:
    """A node in the GitNexus knowledge graph."""

    id: str
    label: str
    properties: dict[str, object] = field(default_factory=dict)


@dataclass
class GraphRelationship:
    """An edge in the GitNexus knowledge graph."""

    id: str
    source_id: str
    target_id: str
    type: str
    confidence: float = 1.0
    reason: str = ""


def _parse_node(item: dict) -> GraphNode:
    return GraphNode(
        id=item["id"],
        label=item["label"],
        properties=item.get("properties", {}),
    )


def _parse_relationship(item: dict) -> GraphRelationship:
    return GraphRelationship(
        id=item.get("id", f"{item['sourceId']}->{item['targetId']}"),
        source_id=item["sourceId"],
        target_id=item["targetId"],
        type=item["type"],
        confidence=item.get("confidence", 1.0),
        reason=item.get("reason", ""),
    )


@dataclass
class DependencyGraph:
    """
    A dependency-graph representation parsed from GitNexus output.

    Provides graph lookup methods and computes dependency-level
    metrics for a target file path within the graph.

    Attributes:
        target_file: The file path to compute metrics for.
        nodes: All nodes in the graph, keyed by ID.
        relationships: All relationships in the graph, keyed by ID.
    """

    target_file: str
    nodes: dict[str, GraphNode] = field(default_factory=dict)
    relationships: dict[str, GraphRelationship] = field(default_factory=dict)

    _outgoing: dict[str, list[GraphRelationship]] = field(
        default_factory=lambda: defaultdict(list), repr=False, compare=False
    )
    _incoming: dict[str, list[GraphRelationship]] = field(
        default_factory=lambda: defaultdict(list), repr=False, compare=False
    )

    @property
    def name(self) -> str:
        return "depgraph"

    # ------------------------------------------------------------------
    # Construction
    # ------------------------------------------------------------------

    @classmethod
    def from_gitnexus_dir(
        cls, gitnexus_dir: str | Path, target_file: str
    ) -> DependencyGraph:
        """
        Build a DependencyGraph by loading the LadybugDB JSON store.

        Args:
            gitnexus_dir: Path to the ``.gitnexus/`` directory.
            target_file: The file path to compute metrics for.

        Raises:
            FileNotFoundError: If the graph JSON cannot be found.
        """
        base = Path(gitnexus_dir)
        graph = cls(target_file=target_file)

        lbug_dir = base / "lbug"
        if not lbug_dir.is_dir():
            raise FileNotFoundError(
                f"LadybugDB directory not found at {lbug_dir}. "
                "Install GitNexus (npm install -g gitnexus) and run "
                "'gitnexus analyze' in the repository root first."
            )

        for json_path in lbug_dir.glob("*.json"):
            data = json.loads(json_path.read_text(encoding="utf-8"))
            if isinstance(data, list):
                for item in data:
                    if "label" in item and "id" in item:
                        graph.add_node(_parse_node(item))
                    elif "type" in item and "sourceId" in item:
                        graph.add_relationship(_parse_relationship(item))
            elif isinstance(data, dict):
                for node_data in data.get("nodes", []):
                    graph.add_node(_parse_node(node_data))
                for rel_data in data.get("relationships", []):
                    graph.add_relationship(_parse_relationship(rel_data))

        return graph

    def add_node(self, node: GraphNode) -> None:
        self.nodes[node.id] = node

    def add_relationship(self, rel: GraphRelationship) -> None:
        self.relationships[rel.id] = rel
        self._outgoing[rel.source_id].append(rel)
        self._incoming[rel.target_id].append(rel)

    # ------------------------------------------------------------------
    # Lookups
    # ------------------------------------------------------------------

    def get_node(self, node_id: str) -> GraphNode | None:
        return self.nodes.get(node_id)

    def nodes_of_label(self, label: NodeLabel) -> list[GraphNode]:
        return [n for n in self.nodes.values() if n.label == label]

    def relationships_of_type(self, rel_type: RelationshipType) -> list[GraphRelationship]:
        return [r for r in self.relationships.values() if r.type == rel_type]

    def outgoing(
        self, node_id: str, rel_type: RelationshipType | None = None
    ) -> list[GraphRelationship]:
        rels = self._outgoing.get(node_id, [])
        if rel_type is not None:
            return [r for r in rels if r.type == rel_type]
        return list(rels)

    def incoming(
        self, node_id: str, rel_type: RelationshipType | None = None
    ) -> list[GraphRelationship]:
        rels = self._incoming.get(node_id, [])
        if rel_type is not None:
            return [r for r in rels if r.type == rel_type]
        return list(rels)

    def file_node_id(self) -> str | None:
        """Find the File node ID matching ``target_file``."""
        for node in self.nodes.values():
            if node.label == "File":
                file_path = node.properties.get("filePath", "")
                if isinstance(file_path, str) and (
                    file_path == self.target_file
                    or file_path.endswith(f"/{self.target_file}")
                    or self.target_file.endswith(f"/{file_path}")
                ):
                    return node.id
        return None

    def contained_symbols(self, file_node_id: str) -> list[str]:
        """Return IDs of all symbols directly contained in a file node."""
        return [r.target_id for r in self.outgoing(file_node_id, "CONTAINS")]

    def all_contained_symbols(self, node_id: str) -> list[str]:
        """Return IDs of all symbols transitively reachable via CONTAINS edges.

        Performs a BFS down the CONTAINS tree starting from *node_id*.
        Cycles are handled safely via a visited set.
        """
        from collections import deque

        visited: set[str] = set()
        result: list[str] = []
        frontier: deque[str] = deque(self.contained_symbols(node_id))
        while frontier:
            child = frontier.popleft()
            if child in visited:
                continue
            visited.add(child)
            result.append(child)
            frontier.extend(self.contained_symbols(child))
        return result

    # ------------------------------------------------------------------
    # Metrics
    # ------------------------------------------------------------------

    def metrics(self) -> dict[str, float]:
        from topos.metrics.depgraph.coupling import (
            calculate_coupling,
            calculate_dependency_depth,
        )
        from topos.metrics.depgraph.fan import calculate_fan_in_out

        file_id = self.file_node_id()
        if file_id is None:
            return {
                "depgraph.coupling": 0.0,
                "depgraph.instability": 0.5,
                "depgraph.fan_in": 0.0,
                "depgraph.fan_out": 0.0,
                "depgraph.dep_depth": 0.0,
            }

        symbol_ids = set(self.all_contained_symbols(file_id))
        symbol_ids.add(file_id)
        coupling_result = calculate_coupling(self, file_id, symbol_ids)
        instability = (
            coupling_result.efferent / coupling_result.total
            if coupling_result.total > 0
            else 0.5
        )
        fan_result = calculate_fan_in_out(self, file_id, symbol_ids)
        dep_depth = calculate_dependency_depth(self, file_id)

        return {
            "depgraph.coupling": float(coupling_result.total),
            "depgraph.instability": instability,
            "depgraph.fan_in": float(fan_result.fan_in),
            "depgraph.fan_out": float(fan_result.fan_out),
            "depgraph.dep_depth": float(dep_depth),
        }
