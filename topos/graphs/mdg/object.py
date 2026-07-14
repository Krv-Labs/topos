"""Module Dependency Graph — Ladybug loader with Rust metrics."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path
from typing import Literal

from topos.topos_functors import (
    GraphNode as RustGraphNode,
)
from topos.topos_functors import (
    GraphRelationship as RustGraphRelationship,
)
from topos.topos_functors import (
    LadybugSchemaMismatchError,
)
from topos.topos_functors import (
    ModuleDependencyGraph as RustModuleDependencyGraph,
)

__all__ = [
    "GraphNode",
    "GraphRelationship",
    "LadybugSchemaMismatchError",
    "ModuleDependencyGraph",
]


@dataclass
class GraphNode:
    id: str
    label: str
    properties: dict[str, object] = field(default_factory=dict)


@dataclass
class GraphRelationship:
    id: str
    source_id: str
    target_id: str
    type: str
    confidence: float = 1.0
    reason: str = ""
    properties: dict[str, object] = field(default_factory=dict)


class LadybugBranchMismatchError(FileNotFoundError):
    """Raised when ``.gitnexus`` has indexed stores, but none for the current branch.

    Subclasses ``FileNotFoundError`` (same trick ``LadybugSchemaMismatchError``
    plays on ``RuntimeError``) so every existing ``except FileNotFoundError``
    elsewhere in the codebase keeps degrading gracefully with no further
    changes -- this only needs special-casing where a *better* message/state
    is wanted, not everywhere graceful degradation already happens.
    """


def _is_shadow_replay_error(exc: BaseException) -> bool:
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


def _load_ladybugdb(lbug_path: Path, target_file: str) -> RustModuleDependencyGraph:
    import ladybug as lb

    nodes: list[GraphNode] = []
    relationships: list[GraphRelationship] = []
    try:
        db = lb.Database(str(lbug_path), read_only=True)
    except RuntimeError as exc:
        if not _is_shadow_replay_error(exc):
            _raise_schema_mismatch(exc)
        try:
            db = lb.Database(str(lbug_path), read_only=False)
        except RuntimeError as retry_exc:
            _raise_schema_mismatch(retry_exc)
    conn = lb.Connection(db)

    tables_result = conn.execute("CALL show_tables() RETURN *")
    node_tables = []
    while tables_result.has_next():
        row = tables_result.get_next()
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
            nodes.append(GraphNode(id=node_id, label=label, properties=props))

    # Load all relationships from the single CodeRelation table. Also try
    # to pull `step` (the 1-indexed STEP_IN_PROCESS ordering property, see
    # topos/graphs/process/object.py) -- not every ladybug schema version
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
        relationships.append(
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

    return _to_rust(target_file, nodes, relationships)


def _load_json_dir(lbug_dir: Path, target_file: str) -> RustModuleDependencyGraph:
    nodes: list[GraphNode] = []
    relationships: list[GraphRelationship] = []
    for json_path in lbug_dir.glob("*.json"):
        data = json.loads(json_path.read_text(encoding="utf-8"))
        if isinstance(data, list):
            for item in data:
                if "label" in item and "id" in item:
                    nodes.append(_parse_node(item))
                elif "type" in item and "sourceId" in item:
                    relationships.append(_parse_relationship(item))
        elif isinstance(data, dict):
            for node_data in data.get("nodes", []):
                nodes.append(_parse_node(node_data))
            for rel_data in data.get("relationships", []):
                relationships.append(_parse_relationship(rel_data))
    return _to_rust(target_file, nodes, relationships)


def _to_rust(
    target_file: str,
    nodes: list[GraphNode],
    relationships: list[GraphRelationship],
) -> RustModuleDependencyGraph:
    rust_nodes = [
        RustGraphNode(
            id=n.id,
            label=n.label,
            properties={k: str(v) for k, v in n.properties.items()},
        )
        for n in nodes
    ]
    rust_rels = [
        RustGraphRelationship(
            id=r.id,
            source_id=r.source_id,
            target_id=r.target_id,
            type=r.type,
            confidence=r.confidence,
            reason=r.reason,
        )
        for r in relationships
    ]
    return RustModuleDependencyGraph.from_parts(target_file, rust_nodes, rust_rels)


def from_gitnexus_dir(
    gitnexus_dir: str | Path, target_file: str
) -> RustModuleDependencyGraph:
    """
    Build a ModuleDependencyGraph from a ``.gitnexus/`` directory.

    Supports both the current LadybugDB binary format (``lbug`` file,
    produced by GitNexus >= 1.5) and the legacy JSON directory format, and
    resolves the branch-scoped store GitNexus writes when the flat slot
    doesn't match the current branch (see
    :func:`topos.utils.gitnexus.resolve_lbug_store`).

    Args:
        gitnexus_dir: Path to the ``.gitnexus/`` directory.
        target_file: The file path to compute metrics for.

    Raises:
        FileNotFoundError: If the graph store cannot be found.
        LadybugBranchMismatchError: If stores are indexed under
            ``gitnexus_dir``, but none for the current branch.
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
            "Install GitNexus (npm install -g gitnexus) and run "
            "'gitnexus analyze' in the repository root first."
        )

    if lbug_path.is_file():
        return _load_ladybugdb(lbug_path, target_file)
    if lbug_path.is_dir():
        return _load_json_dir(lbug_path, target_file)

    raise FileNotFoundError(
        f"LadybugDB store not found at {lbug_path}. "
        "Install GitNexus (npm install -g gitnexus) and run "
        "'gitnexus analyze' in the repository root first."
    )


def get_node(self: RustModuleDependencyGraph, node_id: str) -> RustGraphNode | None:
    return self.nodes.get(node_id)


def nodes_of_label(
    self: RustModuleDependencyGraph, label: str
) -> list[RustGraphNode]:
    return [n for n in self.nodes.values() if n.label == label]


def relationships_of_type(
    self: RustModuleDependencyGraph, rel_type: RelationshipType
) -> list[RustGraphRelationship]:
    return [r for r in self.relationships.values() if r.type == rel_type]


def outgoing(
    self: RustModuleDependencyGraph,
    node_id: str,
    rel_type: RelationshipType | None = None,
) -> list[RustGraphRelationship]:
    rels = [r for r in self.relationships.values() if r.source_id == node_id]
    if rel_type is not None:
        rels = [r for r in rels if r.type == rel_type]
    return rels


def incoming(
    self: RustModuleDependencyGraph,
    node_id: str,
    rel_type: RelationshipType | None = None,
) -> list[RustGraphRelationship]:
    rels = [r for r in self.relationships.values() if r.target_id == node_id]
    if rel_type is not None:
        rels = [r for r in rels if r.type == rel_type]
    return rels


def contained_symbols(self: RustModuleDependencyGraph, file_node_id: str) -> list[str]:
    """Return IDs of all symbols directly contained in a file node."""
    return [r.target_id for r in outgoing(self, file_node_id, "CONTAINS")]


def all_contained_symbols(self: RustModuleDependencyGraph, node_id: str) -> list[str]:
    """Return IDs of all symbols transitively reachable via CONTAINS edges.

    Performs a BFS down the CONTAINS tree starting from *node_id*. Cycles
    are handled safely via a visited set.
    """
    from collections import deque

    visited: set[str] = set()
    result: list[str] = []
    frontier: deque[str] = deque(contained_symbols(self, node_id))
    while frontier:
        child = frontier.popleft()
        if child in visited:
            continue
        visited.add(child)
        result.append(child)
        frontier.extend(contained_symbols(self, child))
    return result


ModuleDependencyGraph = RustModuleDependencyGraph
ModuleDependencyGraph.from_gitnexus_dir = staticmethod(from_gitnexus_dir)  # type: ignore[method-assign]
# ponytail: the Rust binding (crates/topos-pyo3/src/graphs.rs) only exposes
# `nodes`/`relationships` dict getters plus the metrics fast-path; the
# lookup/traversal helpers below recompute their index from those dicts on
# every call (no cached _outgoing/_incoming like the old pure-Python class --
# PyO3 classes without `#[pyclass(dict)]` can't hold extra instance state)
# rather than fully faithful, cached graph traversal. Fine for the current
# call sites (single-file, on-demand lookups: fan/coupling probes, the
# advisory curvature probe, refactor's process-graph filter); upgrade to
# native pyo3 methods with a cached adjacency index if a hot loop over many
# files makes the O(E)-per-call cost measurable.
ModuleDependencyGraph.get_node = get_node  # type: ignore[method-assign]
ModuleDependencyGraph.nodes_of_label = nodes_of_label  # type: ignore[method-assign]
ModuleDependencyGraph.relationships_of_type = relationships_of_type  # type: ignore[method-assign]
ModuleDependencyGraph.outgoing = outgoing  # type: ignore[method-assign]
ModuleDependencyGraph.incoming = incoming  # type: ignore[method-assign]
ModuleDependencyGraph.contained_symbols = contained_symbols  # type: ignore[method-assign]
ModuleDependencyGraph.all_contained_symbols = all_contained_symbols  # type: ignore[method-assign]
