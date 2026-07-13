"""Tests for the process-graph directed FRC probe (issue #86)."""

from __future__ import annotations

from topos.functors.probes.process.curvature import calculate_process_curvature
from topos.graphs.process.object import ProcessGraph, ProcessPath, ProcessStep


def _path(process_id: str, node_ids: list[str]) -> ProcessPath:
    return ProcessPath(
        process_id=process_id,
        steps=[
            ProcessStep(node_id=nid, label="Function", step=i)
            for i, nid in enumerate(node_ids)
        ],
    )


def test_bowtie_quiver_bridge_is_most_negative():
    # Four paths fan into "hub", one bridge hub->bridge, four paths fan out.
    paths = [_path(f"p{i}", [f"in{i}", "hub", "bridge", f"out{i}"]) for i in range(4)]
    graph = ProcessGraph(target_file="f.py", paths=paths)

    result = calculate_process_curvature(graph)
    curvature_by_pair = {(src, dst): c for src, dst, c in result.edges}
    bridge_curvature = curvature_by_pair[("hub", "bridge")]
    for (src, dst), curvature in curvature_by_pair.items():
        if (src, dst) != ("hub", "bridge"):
            assert bridge_curvature < curvature


def test_directed_cycle_uniform_curvature():
    node_ids = ["n0", "n1", "n2", "n3", "n0"]
    graph = ProcessGraph(target_file="f.py", paths=[_path("p0", node_ids)])
    result = calculate_process_curvature(graph)
    curvatures = [c for _, _, c in result.edges]
    assert len(curvatures) == 4
    assert all(abs(c - curvatures[0]) < 1e-9 for c in curvatures)


def test_empty_graph_returns_no_edges():
    graph = ProcessGraph(target_file="f.py", paths=[])
    result = calculate_process_curvature(graph)
    assert result.edges == []
