"""Tests for the MDG balanced Forman curvature probe (issue #84)."""

from __future__ import annotations

from topos.functors.probes.mdg.curvature import calculate_mdg_curvature
from topos.graphs.mdg.object import GraphNode, GraphRelationship, ModuleDependencyGraph


def _file(mdg: ModuleDependencyGraph, file_id: str) -> None:
    mdg.add_node(GraphNode(id=file_id, label="File", properties={"filePath": file_id}))


def _imports(mdg: ModuleDependencyGraph, idx: int, source: str, target: str) -> None:
    mdg.add_relationship(
        GraphRelationship(
            id=f"imp{idx}", source_id=source, target_id=target, type="IMPORTS"
        )
    )


def test_bridge_between_two_hub_clusters_is_most_negative():
    # Two triangles (a-b-c) and (d-e-f), connected only by a single bridge c->d.
    mdg = ModuleDependencyGraph(target_file="a.py")
    for f in ("a.py", "b.py", "c.py", "d.py", "e.py", "f.py"):
        _file(mdg, f)

    edges = [
        ("a.py", "b.py"),
        ("b.py", "c.py"),
        ("c.py", "a.py"),
        ("d.py", "e.py"),
        ("e.py", "f.py"),
        ("f.py", "d.py"),
        ("c.py", "d.py"),
    ]
    for idx, (source, target) in enumerate(edges):
        _imports(mdg, idx, source, target)

    result = calculate_mdg_curvature(mdg, "c.py")
    curvature_by_pair = {
        frozenset((src, dst)): curvature for src, dst, curvature in result.edges
    }
    bridge_curvature = curvature_by_pair[frozenset(("c.py", "d.py"))]
    triangle_curvature = curvature_by_pair[frozenset(("a.py", "c.py"))]
    assert bridge_curvature < triangle_curvature


def test_unknown_file_returns_empty():
    mdg = ModuleDependencyGraph(target_file="a.py")
    _file(mdg, "a.py")
    result = calculate_mdg_curvature(mdg, "nonexistent.py")
    assert result.edges == []


def test_not_folded_into_metrics():
    mdg = ModuleDependencyGraph(target_file="a.py")
    _file(mdg, "a.py")
    _file(mdg, "b.py")
    _imports(mdg, 0, "a.py", "b.py")
    m = mdg.metrics()
    assert not any("curvature" in k for k in m)
