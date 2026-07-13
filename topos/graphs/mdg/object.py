"""Module Dependency Graph — Ladybug loader with Rust metrics."""

from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path

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

    result = conn.execute(
        "MATCH (src)-[r:CodeRelation]->(dst) "
        "RETURN src.id, dst.id, r.type, r.confidence, r.reason"
    )
    idx = 0
    while result.has_next():
        src_id, dst_id, rel_type, confidence, reason = result.get_next()
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
    base = Path(gitnexus_dir)
    lbug_path = base / "lbug"
    if lbug_path.is_file():
        return _load_ladybugdb(lbug_path, target_file)
    if lbug_path.is_dir():
        return _load_json_dir(lbug_path, target_file)
    try:
        return RustModuleDependencyGraph.from_gitnexus_dir(
            str(gitnexus_dir), target_file
        )
    except Exception:
        raise FileNotFoundError(
            f"LadybugDB store not found at {lbug_path}. "
            "Install GitNexus (npm install -g gitnexus) and run "
            "'gitnexus analyze' in the repository root first."
        ) from None


ModuleDependencyGraph = RustModuleDependencyGraph
ModuleDependencyGraph.from_gitnexus_dir = staticmethod(from_gitnexus_dir)  # type: ignore[method-assign]
