"""Tests for the ProcessGraph representation (issue #86)."""

import json
import tempfile
from pathlib import Path

from topos.graphs.mdg.object import GraphNode, GraphRelationship, ModuleDependencyGraph
from topos.graphs.process.object import ProcessGraph


def _mdg_with_process() -> ModuleDependencyGraph:
    mdg = ModuleDependencyGraph(target_file="f.py")
    mdg.add_node(GraphNode(id="proc1", label="Process"))
    mdg.add_node(GraphNode(id="fnA", label="Function", properties={"filePath": "f.py"}))
    mdg.add_node(GraphNode(id="fnB", label="Function", properties={"filePath": "g.py"}))
    mdg.add_node(GraphNode(id="fnC", label="Function", properties={"filePath": "h.py"}))
    mdg.add_node(GraphNode(id="fileF", label="File", properties={"filePath": "f.py"}))
    mdg.add_relationship(
        GraphRelationship(id="c1", source_id="fileF", target_id="fnA", type="CONTAINS")
    )
    # Intentionally added out of step order to verify sorting.
    mdg.add_relationship(
        GraphRelationship(
            id="s2",
            source_id="proc1",
            target_id="fnB",
            type="STEP_IN_PROCESS",
            properties={"step": 2},
        )
    )
    mdg.add_relationship(
        GraphRelationship(
            id="s1",
            source_id="proc1",
            target_id="fnA",
            type="STEP_IN_PROCESS",
            properties={"step": 1},
        )
    )
    mdg.add_relationship(
        GraphRelationship(
            id="s3",
            source_id="proc1",
            target_id="fnC",
            type="STEP_IN_PROCESS",
            properties={"step": 3},
        )
    )
    return mdg


def test_from_mdg_orders_steps_ascending():
    mdg = _mdg_with_process()
    graph = ProcessGraph.from_mdg(mdg, "f.py")

    assert len(graph.paths) == 1
    path = graph.paths[0]
    assert path.process_id == "proc1"
    assert [s.node_id for s in path.steps] == ["fnA", "fnB", "fnC"]
    assert [s.step for s in path.steps] == [1, 2, 3]


def test_from_mdg_falls_back_to_discovery_order_without_step_property():
    mdg = ModuleDependencyGraph(target_file="f.py")
    mdg.add_node(GraphNode(id="proc1", label="Process"))
    mdg.add_node(GraphNode(id="fnA", label="Function"))
    mdg.add_node(GraphNode(id="fnB", label="Function"))
    mdg.add_relationship(
        GraphRelationship(
            id="s1", source_id="proc1", target_id="fnA", type="STEP_IN_PROCESS"
        )
    )
    mdg.add_relationship(
        GraphRelationship(
            id="s2", source_id="proc1", target_id="fnB", type="STEP_IN_PROCESS"
        )
    )

    graph = ProcessGraph.from_mdg(mdg, "f.py")
    assert len(graph.paths) == 1
    assert [s.node_id for s in graph.paths[0].steps] == ["fnA", "fnB"]


def test_from_mdg_falls_back_to_discovery_order_for_nonnumeric_step():
    mdg = ModuleDependencyGraph(target_file="f.py")
    mdg.add_node(GraphNode(id="proc1", label="Process"))
    mdg.add_node(GraphNode(id="fnA", label="Function"))
    mdg.add_relationship(
        GraphRelationship(
            id="s1",
            source_id="proc1",
            target_id="fnA",
            type="STEP_IN_PROCESS",
            properties={"step": "first"},
        )
    )

    graph = ProcessGraph.from_mdg(mdg, "f.py")
    assert graph.paths[0].steps[0].step == 0


def test_edges_flattens_consecutive_steps():
    mdg = _mdg_with_process()
    graph = ProcessGraph.from_mdg(mdg, "f.py")

    assert graph.edges() == [("fnA", "fnB"), ("fnB", "fnC")]


def test_paths_touching_file_filters_by_containment():
    mdg = _mdg_with_process()
    graph = ProcessGraph.from_mdg(mdg, "f.py")

    touching = graph.paths_touching_file("fileF")
    assert [p.process_id for p in touching] == ["proc1"]

    not_touching = graph.paths_touching_file("nonexistent")
    assert not_touching == []


def test_no_process_nodes_yields_empty_paths():
    mdg = ModuleDependencyGraph(target_file="f.py")
    mdg.add_node(GraphNode(id="fnA", label="Function"))
    graph = ProcessGraph.from_mdg(mdg, "f.py")
    assert graph.paths == []
    assert graph.edges() == []



def test_from_gitnexus_dir_delegates_to_branch_aware_mdg_resolution():
    """ProcessGraph.from_gitnexus_dir just wraps
    ModuleDependencyGraph.from_gitnexus_dir + from_mdg -- confirm the
    delegation actually resolves a branch-scoped store correctly (not just
    that it forwards the call), rather than re-deriving the resolver's own
    unit tests here."""
    with tempfile.TemporaryDirectory() as tmp:
        project_root = Path(tmp)
        gitnexus_dir = project_root / ".gitnexus"

        git_dir = project_root / ".git"
        git_dir.mkdir()
        (git_dir / "HEAD").write_text("ref: refs/heads/feature-x\n", encoding="utf-8")

        flat_lbug = gitnexus_dir / "lbug"
        flat_lbug.mkdir(parents=True)
        (flat_lbug / "graph.json").write_text(
            json.dumps(
                [
                    {
                        "id": "File:main.py",
                        "label": "File",
                        "properties": {"filePath": "main.py"},
                    }
                ]
            ),
            encoding="utf-8",
        )
        (gitnexus_dir / "meta.json").write_text(
            json.dumps({"branch": "main"}), encoding="utf-8"
        )

        branch_dir = gitnexus_dir / "branches" / "feature-x-deadbeef"
        branch_lbug = branch_dir / "lbug"
        branch_lbug.mkdir(parents=True)
        (branch_lbug / "graph.json").write_text(
            json.dumps(
                [
                    {
                        "id": "File:feature.py",
                        "label": "File",
                        "properties": {"filePath": "feature.py"},
                    }
                ]
            ),
            encoding="utf-8",
        )
        (branch_dir / "meta.json").write_text(
            json.dumps({"branch": "feature-x"}), encoding="utf-8"
        )

        pg = ProcessGraph.from_gitnexus_dir(gitnexus_dir, target_file="feature.py")

    assert pg._mdg is not None
    assert pg._mdg.get_node("File:feature.py") is not None
    assert pg._mdg.get_node("File:main.py") is None
