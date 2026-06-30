"""Tests for the balanced Forman-Ricci curvature probe (Topping 2022)."""

from __future__ import annotations

import pytest

from topos.topos_functors import calculate_balanced_frc


# ---------------------------------------------------------------------------
# Rust kernel — formula correctness
# ---------------------------------------------------------------------------


def _frc(n: int, edges: list[tuple[int, int]]) -> dict[tuple[int, int], float]:
    """Helper: return {(u,v): ric} for every result edge."""
    return {(e.source_idx, e.target_idx): e.ric for e in calculate_balanced_frc(n, edges)}


def test_empty_graph_returns_empty():
    assert calculate_balanced_frc(0, []) == []
    assert calculate_balanced_frc(5, []) == []


def test_linear_chain_leaf_edges_positive():
    # A(0)—B(1)—C(2): d_A=1, d_B=2, d_C=1, no triangles.
    # Ric(A,B) = 2/1 + 2/2 − 2 + 0 = 1.0  (leaf edge — positive curvature)
    results = calculate_balanced_frc(3, [(0, 1), (1, 2)])
    assert len(results) == 2
    for e in results:
        assert abs(e.ric - 1.0) < 1e-9
        assert not e.is_bridge


def test_triangle_all_edges_positive():
    # All nodes have d=2, triangles=1 per edge.
    # Ric = 2/2 + 2/2 − 2 + 1*(1/2+1/2) = 1.0
    results = calculate_balanced_frc(3, [(0, 1), (1, 2), (0, 2)])
    assert len(results) == 3
    for e in results:
        assert abs(e.ric - 1.0) < 1e-9
        assert not e.is_bridge


def test_bridge_between_two_triangles_negative():
    # Nodes 0,1,2 triangle; nodes 3,4,5 triangle; bridge 0–3.
    # d_0=3, d_3=3, t=0 → Ric = 2/3+2/3−2 = −2/3 ≈ −0.667
    edges = [(0, 1), (1, 2), (0, 2), (3, 4), (4, 5), (3, 5), (0, 3)]
    results = calculate_balanced_frc(6, edges)
    ric_map = {(e.source_idx, e.target_idx): e.ric for e in results}
    bridge_ric = ric_map.get((0, 3), ric_map.get((3, 0)))
    assert bridge_ric is not None
    assert abs(bridge_ric - (2 / 3 + 2 / 3 - 2)) < 1e-9
    assert bridge_ric < 0
    # d=3 nodes → |Ric|=0.667 < 1.0 threshold → not flagged as bridge
    bridge_entry = next(
        e for e in results if {e.source_idx, e.target_idx} == {0, 3}
    )
    assert not bridge_entry.is_bridge


def test_high_degree_bridge_is_flagged():
    # Node 0 connects to 1–9 (d=10); node 10 connects to 11–19 (d=10); bridge 0–10.
    # Ric = 2/10+2/10−2 = −1.6 < −1.0 threshold → is_bridge=True
    edges = [(0, i) for i in range(1, 10)]
    edges += [(10, i) for i in range(11, 20)]
    edges.append((0, 10))
    results = calculate_balanced_frc(20, edges)
    bridge = next(e for e in results if {e.source_idx, e.target_idx} == {0, 10})
    assert abs(bridge.ric - (2 / 10 + 2 / 10 - 2)) < 1e-9
    assert bridge.is_bridge


def test_triangle_term_raises_curvature():
    # Show that adding shared neighbours (triangles) raises the curvature score.
    #
    # No-triangle case (n=10): A=0 connects to private leaves 2,3,4,5; B=1 to 6,7,8,9.
    # d_A=5, d_B=5, t=0 → Ric = 2/5+2/5−2 = −1.2
    edges_no_triangle = (
        [(0, i) for i in range(2, 6)]   # A's private leaves: 2,3,4,5
        + [(1, i) for i in range(6, 10)]  # B's private leaves: 6,7,8,9
        + [(0, 1)]
    )
    res_no = calculate_balanced_frc(10, edges_no_triangle)
    ab_no = next(e for e in res_no if {e.source_idx, e.target_idx} == {0, 1})
    assert abs(ab_no.ric - (-1.2)) < 1e-9
    assert ab_no.is_bridge  # -1.2 < -1.0

    # With-triangle case (n=9): A=0 and B=1 share nodes 2,3,4; A has private 5,6; B has 7,8.
    # d_A=6, d_B=6, t=3 → Ric = 2/6+2/6−2 + 3*(1/6+1/6) = −4/3 + 1 = −1/3 ≈ −0.333
    edges_with = (
        [(0, i) for i in range(2, 5)]   # shared neighbours 2,3,4
        + [(1, i) for i in range(2, 5)]
        + [(0, 5), (0, 6)]              # A-only leaves
        + [(1, 7), (1, 8)]              # B-only leaves
        + [(0, 1)]
    )
    res_with = calculate_balanced_frc(9, edges_with)
    ab_with = next(e for e in res_with if {e.source_idx, e.target_idx} == {0, 1})
    assert abs(ab_with.ric - (-1 / 3)) < 1e-9
    assert not ab_with.is_bridge  # -0.333 > -1.0
    assert ab_with.ric > ab_no.ric, "triangles must raise curvature"


# ---------------------------------------------------------------------------
# Python probe — MDG integration
# ---------------------------------------------------------------------------


def _make_graph(file_edges: list[tuple[str, str]]) -> object:
    """Build a minimal ModuleDependencyGraph with File nodes and IMPORTS edges."""
    from topos.graphs.mdg.object import GraphNode, GraphRelationship, ModuleDependencyGraph

    graph = ModuleDependencyGraph(target_file="a.py")

    # Collect unique paths → File nodes.
    paths = {p for pair in file_edges for p in pair}
    path_to_id: dict[str, str] = {p: f"file_{i}" for i, p in enumerate(sorted(paths))}
    for path, nid in path_to_id.items():
        graph.add_node(GraphNode(id=nid, label="File", properties={"filePath": path}))

    for i, (src, tgt) in enumerate(file_edges):
        graph.add_relationship(
            GraphRelationship(
                id=f"rel_{i}",
                source_id=path_to_id[src],
                target_id=path_to_id[tgt],
                type="IMPORTS",
            )
        )
    return graph


def test_probe_returns_empty_for_single_file():
    from topos.functors.probes.mdg.curvature import mdg_edge_curvatures

    graph = _make_graph([])  # no edges, no File nodes except none
    assert mdg_edge_curvatures(graph) == []


def test_probe_symmetrizes_directed_edges():
    """A→B and B→A should collapse to one undirected edge."""
    from topos.functors.probes.mdg.curvature import mdg_edge_curvatures

    graph = _make_graph([("a.py", "b.py"), ("b.py", "a.py")])
    results = mdg_edge_curvatures(graph)
    assert len(results) == 1


def test_probe_bridge_edge_flagged():
    """Linear chain a→b→c: b's edges are leaf-adjacent (d=1 on ends), positive.
    But a three-module chain where outer modules have extra connections gives a
    clear bridge at b–c when a and c are unconnected to each other.
    Use a high-degree bridge: build two "clusters" each with 3 files connected
    to a hub, plus one bridge between the hubs."""
    from topos.functors.probes.mdg.curvature import mdg_edge_curvatures

    # Hub A connects to leaves a1,a2,a3,a4; hub B connects to b1,b2,b3,b4; bridge A→B.
    edges = (
        [("hub_a.py", f"a{i}.py") for i in range(1, 5)]
        + [("hub_b.py", f"b{i}.py") for i in range(1, 5)]
        + [("hub_a.py", "hub_b.py")]
    )
    graph = _make_graph(edges)
    results = mdg_edge_curvatures(graph)

    bridge = next(
        (
            e
            for e in results
            if {"hub_a.py", "hub_b.py"} == {e.source, e.target}
        ),
        None,
    )
    assert bridge is not None, "bridge edge not found in results"
    # d_hub_a = 5 (4 leaves + 1 bridge), d_hub_b = 5, t=0
    # Ric = 2/5 + 2/5 − 2 = −1.2 → is_bridge = True
    assert bridge.ric < -1.0
    assert bridge.is_bridge


def test_probe_paths_mapped_correctly():
    """Source/target in result should be file path strings, not node IDs."""
    from topos.functors.probes.mdg.curvature import mdg_edge_curvatures

    graph = _make_graph([("foo/bar.py", "foo/baz.py")])
    results = mdg_edge_curvatures(graph)
    assert len(results) == 1
    paths = {results[0].source, results[0].target}
    assert paths == {"foo/bar.py", "foo/baz.py"}
