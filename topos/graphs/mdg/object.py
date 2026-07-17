"""
Module Dependency Graph (MDG) Representation
============================================
Consumes the knowledge graph produced by `GitNexus
<https://github.com/abhigyanpatwari/GitNexus>`_ and lifts it into a
:class:`~topos.graphs.base.Representation`.  This is the **inter-module**
view of the program — it captures the import/call/inheritance structure
across files, packages, and classes.  Compare this with the academic
**intra-procedural Program Dependence Graph** at
:mod:`topos.graphs.pdg.object`, which records control- and data-dependence
edges *within* a single procedure.

GitNexus runs ``gitnexus analyze`` on a repository and writes a
``.gitnexus/`` directory containing a LadybugDB graph store.  This module
parses that output into an in-memory graph of typed nodes and
relationships, then computes dependency-level metrics that the AST alone
cannot provide.

Node labels and relationship types mirror GitNexus's shared schema::

    Nodes:  File, Module, Function, Class, Method, Import, ...
    Edges:  CALLS, IMPORTS, INHERITS, CONTAINS, USES, ...

Metrics produced (feed the COMPOSABLE generator of H(G_qual)):
    - ``mdg.coupling``   -- afferent + efferent coupling for a file
    - ``mdg.instability`` -- Ce / (Ca + Ce)  (Martin's metric)
    - ``mdg.fan_in``      -- incoming CALLS edges
    - ``mdg.fan_out``     -- outgoing CALLS edges
    - ``mdg.dep_depth``   -- longest IMPORTS chain from the file

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


class LadybugSchemaMismatchError(RuntimeError):
    """Raised when ``.gitnexus/lbug`` storage version exceeds embedded ladybug."""

    def __init__(self, message: str, *, original: Exception | None = None) -> None:
        super().__init__(message)
        self.original = original


class LadybugBranchMismatchError(FileNotFoundError):
    """Raised when ``.gitnexus`` has indexed stores, but none for the current branch.

    Subclasses ``FileNotFoundError`` (same trick ``LadybugSchemaMismatchError``
    plays on ``RuntimeError``) so every existing ``except FileNotFoundError``
    elsewhere in the codebase keeps degrading gracefully with no further
    changes -- this only needs special-casing where a *better* message/state
    is wanted, not everywhere graceful degradation already happens.
    """


def _is_shadow_replay_error(exc: BaseException) -> bool:
    """Whether *exc* is Ladybug refusing to replay shadow pages read-only.

    Incrementally-updated ``.gitnexus`` stores can be left with pending
    shadow pages (e.g. after ``gitnexus analyze`` without a full wipe).
    Ladybug requires a read-write handle to replay them.
    """
    msg = str(exc).lower()
    return "shadow page" in msg and "read-only" in msg


def _raise_schema_mismatch(exc: BaseException) -> None:
    msg = str(exc).lower()
    if "different version" not in msg and "storage version" not in msg:
        raise exc
    raise LadybugSchemaMismatchError(
        "LadybugDB storage version mismatch while loading .gitnexus/lbug. "
        "Upgrade Topos to v0.3.4+ (bundles ladybug 0.17+) or re-run "
        "'topos depgraph generate' after upgrading. "
        "GitNexus 1.6.x requires ladybug 0.17+.",
        original=exc if isinstance(exc, Exception) else None,
    ) from exc


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
    properties: dict[str, object] = field(default_factory=dict)


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
        properties=item.get("properties", {}),
    )


@dataclass
class ModuleDependencyGraph:
    """
    Inter-module dependency-graph representation parsed from GitNexus output.

    Provides graph lookup methods and computes dependency-level
    metrics for a target file path within the graph.

    This is the **module-level** dependency view (imports, calls, inheritance
    across files).  It is distinct from the academic intra-procedural
    Program Dependence Graph at :mod:`topos.graphs.pdg.object`.

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
        return "mdg"

    @property
    def dimension(self) -> str:
        # Feeds the COMPOSABLE generator of H(G_qual).
        return "composable"

    # ------------------------------------------------------------------
    # Construction
    # ------------------------------------------------------------------

    @classmethod
    def from_gitnexus_dir(
        cls, gitnexus_dir: str | Path, target_file: str
    ) -> ModuleDependencyGraph:
        """
        Build a ModuleDependencyGraph from a ``.gitnexus/`` directory.

        Supports both the current LadybugDB binary format (``lbug`` file,
        produced by GitNexus ≥ 1.5) and the legacy JSON directory format.

        Args:
            gitnexus_dir: Path to the ``.gitnexus/`` directory.
            target_file: The file path to compute metrics for.

        Raises:
            FileNotFoundError: If the graph store cannot be found.
            ImportError: If ``ladybug`` is not installed for binary format.
            LadybugSchemaMismatchError: If the store version exceeds embedded ladybug.
        """
        from topos.utils.gitnexus import current_git_branch, resolve_lbug_store

        base = Path(gitnexus_dir)
        branch = current_git_branch(base.parent)
        resolved = resolve_lbug_store(base, branch)
        lbug_path = resolved.path

        if lbug_path is None:
            if resolved.available_branches:
                raise LadybugBranchMismatchError(
                    f"No GitNexus store indexed for branch {branch!r} at {base}. "
                    f"Indexed branches: {', '.join(resolved.available_branches)}. "
                    "Run 'gitnexus analyze' on this branch (or 'topos depgraph "
                    "generate') to index it."
                )
            raise FileNotFoundError(
                f"LadybugDB store not found at {base / 'lbug'}. "
                "Install GitNexus (pnpm add -g gitnexus, or npm install -g gitnexus) and run "
                "'gitnexus analyze' in the repository root first."
            )

        if lbug_path.is_file():
            return cls._from_ladybugdb(lbug_path, target_file)

        if lbug_path.is_dir():
            return cls._from_json_dir(lbug_path, target_file)

        raise FileNotFoundError(
            f"LadybugDB store not found at {lbug_path}. "
            "Install GitNexus (pnpm add -g gitnexus, or npm install -g gitnexus) and run "
            "'gitnexus analyze' in the repository root first."
        )

    @classmethod
    def _from_ladybugdb(
        cls, lbug_path: Path, target_file: str
    ) -> ModuleDependencyGraph:
        """Load from the binary LadybugDB format produced by GitNexus ≥ 1.5."""
        import ladybug as lb

        graph = cls(target_file=target_file)
        try:
            db = lb.Database(str(lbug_path), read_only=True)
        except RuntimeError as exc:
            if not _is_shadow_replay_error(exc):
                _raise_schema_mismatch(exc)
            # Pending shadow pages (e.g. incremental `gitnexus analyze` without
            # a full wipe) can only be replayed with a read-write handle.
            try:
                db = lb.Database(str(lbug_path), read_only=False)
            except RuntimeError as retry_exc:
                _raise_schema_mismatch(retry_exc)
        conn = lb.Connection(db)

        # Discover node tables at runtime so we're not tied to a fixed schema.
        tables_result = conn.execute("CALL show_tables() RETURN *")
        node_tables = []
        while tables_result.has_next():
            row = tables_result.get_next()
            # row: [id, name, type, database, comment]
            if len(row) >= 3 and row[2] == "NODE":
                node_tables.append(row[1])

        for label in node_tables:
            result = conn.execute(f"MATCH (n:`{label}`) RETURN n")
            while result.has_next():
                (node_data,) = result.get_next()
                node_id = node_data.get("id")
                if node_id is None:
                    continue
                props = {k: v for k, v in node_data.items() if not k.startswith("_")}
                graph.add_node(GraphNode(id=node_id, label=label, properties=props))

        # Load all relationships from the single CodeRelation table. Also try
        # to pull `step` (the 1-indexed STEP_IN_PROCESS ordering property, see
        # topos/graphs/process/object.py) — not every ladybug schema version
        # carries this column on CodeRelation, so fall back to the plain query
        # if it isn't there rather than failing the whole load.
        has_step = True
        try:
            result = conn.execute(
                "MATCH (src)-[r:CodeRelation]->(dst) "
                "RETURN src.id, dst.id, r.type, r.confidence, r.reason, r.step"
            )
        except RuntimeError:
            has_step = False
            result = conn.execute(
                "MATCH (src)-[r:CodeRelation]->(dst) "
                "RETURN src.id, dst.id, r.type, r.confidence, r.reason"
            )
        idx = 0
        while result.has_next():
            row = result.get_next()
            if has_step:
                src_id, dst_id, rel_type, confidence, reason, step = row
            else:
                src_id, dst_id, rel_type, confidence, reason = row
                step = None
            if src_id is None or dst_id is None:
                continue
            graph.add_relationship(
                GraphRelationship(
                    id=f"{src_id}->{dst_id}:{rel_type}:{idx}",
                    source_id=src_id,
                    target_id=dst_id,
                    type=rel_type,
                    confidence=confidence or 1.0,
                    reason=reason or "",
                    properties={"step": step} if step is not None else {},
                )
            )
            idx += 1

        return graph

    @classmethod
    def _from_json_dir(cls, lbug_dir: Path, target_file: str) -> ModuleDependencyGraph:
        """Load from the legacy JSON directory format produced by GitNexus < 1.5."""
        graph = cls(target_file=target_file)
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

    def relationships_of_type(
        self, rel_type: RelationshipType
    ) -> list[GraphRelationship]:
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
        from topos.functors.probes.mdg.coupling import (
            calculate_coupling,
            calculate_dependency_depth,
            calculate_instability_from_result,
        )
        from topos.functors.probes.mdg.fan import calculate_fan_in_out

        file_id = self.file_node_id()
        if file_id is None:
            return {
                "mdg.coupling": 0.0,
                "mdg.instability": 0.5,
                "mdg.fan_in": 0.0,
                "mdg.fan_out": 0.0,
                "mdg.dep_depth": 0.0,
            }

        symbol_ids = set(self.all_contained_symbols(file_id))
        symbol_ids.add(file_id)
        coupling_result = calculate_coupling(self, file_id, symbol_ids)
        instability = calculate_instability_from_result(coupling_result)
        fan_result = calculate_fan_in_out(self, file_id, symbol_ids)
        dep_depth = calculate_dependency_depth(self, file_id)

        return {
            "mdg.coupling": float(coupling_result.total),
            "mdg.instability": instability,
            "mdg.fan_in": float(fan_result.fan_in),
            "mdg.fan_out": float(fan_result.fan_out),
            "mdg.dep_depth": float(dep_depth),
        }
