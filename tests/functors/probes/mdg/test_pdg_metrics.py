"""Tests for depgraph-specific metrics (coupling, fan, depth)."""

import json
import tempfile
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest
from topos.functors.probes.mdg.coupling import (
    CouplingResult,
    calculate_coupling,
    calculate_dependency_depth,
    calculate_instability,
    calculate_instability_from_result,
)
from topos.functors.probes.mdg.fan import FanResult, calculate_fan_in_out
from topos.graphs.mdg.object import (
    GraphNode,
    GraphRelationship,
    LadybugSchemaMismatchError,
    ModuleDependencyGraph,
)


def _graph_with_linear_chain() -> ModuleDependencyGraph:
    """A -> B -> C -> D linear import chain, target = A."""
    g = ModuleDependencyGraph(target_file="a.py")
    for name in ("a", "b", "c", "d"):
        g.add_node(
            GraphNode(
                id=f"File:{name}.py",
                label="File",
                properties={"filePath": f"{name}.py"},
            )
        )
    g.add_relationship(
        GraphRelationship(
            id="i1",
            source_id="File:a.py",
            target_id="File:b.py",
            type="IMPORTS",
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="i2",
            source_id="File:b.py",
            target_id="File:c.py",
            type="IMPORTS",
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="i3",
            source_id="File:c.py",
            target_id="File:d.py",
            type="IMPORTS",
        )
    )
    return g


def _graph_with_fan() -> ModuleDependencyGraph:
    """Hub file with many callers and callees."""
    g = ModuleDependencyGraph(target_file="hub.py")
    g.add_node(
        GraphNode(
            id="File:hub.py",
            label="File",
            properties={"filePath": "hub.py"},
        )
    )
    g.add_node(
        GraphNode(
            id="Func:hub:process",
            label="Function",
            properties={"filePath": "hub.py", "name": "process"},
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="c0",
            source_id="File:hub.py",
            target_id="Func:hub:process",
            type="CONTAINS",
        )
    )

    for i in range(5):
        caller_id = f"Func:caller{i}:run"
        g.add_node(
            GraphNode(
                id=caller_id,
                label="Function",
                properties={"filePath": f"caller{i}.py", "name": "run"},
            )
        )
        g.add_relationship(
            GraphRelationship(
                id=f"call_in_{i}",
                source_id=caller_id,
                target_id="Func:hub:process",
                type="CALLS",
            )
        )

    for i in range(3):
        callee_id = f"Func:dep{i}:work"
        g.add_node(
            GraphNode(
                id=callee_id,
                label="Function",
                properties={"filePath": f"dep{i}.py", "name": "work"},
            )
        )
        g.add_relationship(
            GraphRelationship(
                id=f"call_out_{i}",
                source_id="Func:hub:process",
                target_id=callee_id,
                type="CALLS",
            )
        )

    return g


# ---------------------------------------------------------------------------
# Coupling
# ---------------------------------------------------------------------------


def test_coupling_linear_chain():
    g = _graph_with_linear_chain()
    file_id = g.file_node_id()
    assert file_id == "File:a.py"

    result = calculate_coupling(g, file_id)
    assert isinstance(result, CouplingResult)
    assert result.efferent == 1  # a.py imports b.py
    assert result.afferent == 0  # nobody imports a.py
    assert result.total == 1


def test_coupling_result_total():
    r = CouplingResult(afferent=3, efferent=7)
    assert r.total == 10


# ---------------------------------------------------------------------------
# Instability
# ---------------------------------------------------------------------------


def test_instability_all_efferent():
    g = _graph_with_linear_chain()
    instability = calculate_instability(g, "File:a.py")
    assert instability == 1.0  # only efferent, no afferent


def test_instability_zero_coupling():
    g = ModuleDependencyGraph(target_file="isolated.py")
    g.add_node(
        GraphNode(
            id="File:isolated.py",
            label="File",
            properties={"filePath": "isolated.py"},
        )
    )
    instability = calculate_instability(g, "File:isolated.py")
    assert instability == 0.5  # default for zero coupling


def test_instability_from_precomputed_coupling():
    result = CouplingResult(afferent=3, efferent=1)
    instability = calculate_instability_from_result(result)
    assert instability == 0.25


# ---------------------------------------------------------------------------
# Dependency depth
# ---------------------------------------------------------------------------


def test_dependency_depth_linear():
    g = _graph_with_linear_chain()
    depth = calculate_dependency_depth(g, "File:a.py")
    assert depth == 3  # a -> b -> c -> d


def test_dependency_depth_isolated():
    g = ModuleDependencyGraph(target_file="lone.py")
    g.add_node(
        GraphNode(
            id="File:lone.py",
            label="File",
            properties={"filePath": "lone.py"},
        )
    )
    depth = calculate_dependency_depth(g, "File:lone.py")
    assert depth == 0


def test_dependency_depth_cycle():
    """Cycles should not cause infinite loops."""
    g = ModuleDependencyGraph(target_file="x.py")
    g.add_node(
        GraphNode(
            id="File:x.py",
            label="File",
            properties={"filePath": "x.py"},
        )
    )
    g.add_node(
        GraphNode(
            id="File:y.py",
            label="File",
            properties={"filePath": "y.py"},
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="i1",
            source_id="File:x.py",
            target_id="File:y.py",
            type="IMPORTS",
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="i2",
            source_id="File:y.py",
            target_id="File:x.py",
            type="IMPORTS",
        )
    )
    depth = calculate_dependency_depth(g, "File:x.py")
    assert depth == 1


# ---------------------------------------------------------------------------
# Fan-in / Fan-out
# ---------------------------------------------------------------------------


def test_fan_in_out():
    g = _graph_with_fan()
    result = calculate_fan_in_out(g, "File:hub.py")
    assert isinstance(result, FanResult)
    assert result.fan_in == 5
    assert result.fan_out == 3


def test_fan_isolated_file():
    g = ModuleDependencyGraph(target_file="solo.py")
    g.add_node(
        GraphNode(
            id="File:solo.py",
            label="File",
            properties={"filePath": "solo.py"},
        )
    )
    result = calculate_fan_in_out(g, "File:solo.py")
    assert result.fan_in == 0
    assert result.fan_out == 0


# ---------------------------------------------------------------------------
# Integration: depgraph verdicts through the classifier
# ---------------------------------------------------------------------------


def test_classifier_with_depgraph_representation():
    from topos.core.morphism import ProgramMorphism
    from topos.core.omega import EvaluationValue
    from topos.evaluation.characteristic_morphism import CharacteristicMorphism

    source = "def main(): return 1"
    morphism = ProgramMorphism(source=source)
    classifier = CharacteristicMorphism()

    g = _graph_with_linear_chain()
    result = classifier.classify_detailed(morphism, representations=[g])

    assert result.is_parseable
    assert isinstance(result.summary(), EvaluationValue)
    assert "composable" in result.dimensions
    assert "mdg.coupling" in result.raw_metrics


def test_classifier_without_representations_unchanged():
    from topos.core.morphism import ProgramMorphism
    from topos.evaluation.characteristic_morphism import CharacteristicMorphism

    source = "x = 1"
    morphism = ProgramMorphism(source=source)
    classifier = CharacteristicMorphism()

    result_without = classifier.classify_detailed(morphism)
    result_with_none = classifier.classify_detailed(morphism, representations=None)
    result_with_empty = classifier.classify_detailed(morphism, representations=[])

    assert result_without.summary() == result_with_none.summary()
    assert result_without.summary() == result_with_empty.summary()
    # Without extra representations, the AST representation alone feeds
    # the SIMPLE generator.
    assert set(result_without.dimensions.keys()) == {"simple"}


def test_classification_result_str_with_representations():
    from topos.core.omega import EvaluationValue
    from topos.evaluation.characteristic_morphism import ClassificationResult

    result = ClassificationResult(
        is_parseable=True,
        dimensions={"coupling": EvaluationValue.COMPOSABLE},
        scores={"coupling": 0.75},
        lattice_element=EvaluationValue.COMPOSABLE,
        raw_metrics={
            "mdg.coupling": 3.0,
            "mdg.instability": 0.5,
        },
    )
    text = str(result)
    assert "coupling" in text


# ---------------------------------------------------------------------------
# _owning_file edge cases
# ---------------------------------------------------------------------------


def test_owning_file_symbol_with_no_contains_parent():
    """A Function node that has no CONTAINS edge returns None from _owning_file."""
    from topos.functors.probes.mdg.coupling import _owning_file

    g = ModuleDependencyGraph(target_file="orphan.py")
    g.add_node(
        GraphNode(
            id="Func:orphan:stray",
            label="Function",
            properties={"filePath": "orphan.py", "name": "stray"},
        )
    )
    # No CONTAINS relationship pointing at this symbol
    assert _owning_file(g, "Func:orphan:stray") is None


def test_owning_file_unknown_node():
    """Asking for a node_id that doesn't exist in the graph returns None."""
    from topos.functors.probes.mdg.coupling import _owning_file

    g = ModuleDependencyGraph(target_file="x.py")
    assert _owning_file(g, "nonexistent-id") is None


def test_owning_file_via_contains_edge():
    """A Function reachable via a CONTAINS edge resolves to its File owner."""
    from topos.functors.probes.mdg.coupling import _owning_file

    g = ModuleDependencyGraph(target_file="owner.py")
    g.add_node(
        GraphNode(
            id="File:owner.py",
            label="File",
            properties={"filePath": "owner.py"},
        )
    )
    g.add_node(
        GraphNode(
            id="Func:owner:fn",
            label="Function",
            properties={"filePath": "owner.py", "name": "fn"},
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="c1",
            source_id="File:owner.py",
            target_id="Func:owner:fn",
            type="CONTAINS",
        )
    )
    assert _owning_file(g, "Func:owner:fn") == "File:owner.py"


# ---------------------------------------------------------------------------
# outgoing / incoming without rel_type filter
# ---------------------------------------------------------------------------


def test_outgoing_all_types():
    g = _graph_with_linear_chain()
    all_out = g.outgoing("File:a.py")
    assert len(all_out) >= 1
    types = {r.type for r in all_out}
    assert "IMPORTS" in types


def test_incoming_all_types():
    g = _graph_with_linear_chain()
    all_in = g.incoming("File:b.py")
    assert len(all_in) >= 1
    types = {r.type for r in all_in}
    assert "IMPORTS" in types


def test_outgoing_missing_node_returns_empty():
    g = ModuleDependencyGraph(target_file="empty.py")
    assert g.outgoing("no-such-node") == []
    assert g.incoming("no-such-node") == []


# ---------------------------------------------------------------------------
# ModuleDependencyGraph.from_gitnexus_dir — list-format JSON
# ---------------------------------------------------------------------------


def _write_lbug_dir(base: Path, data: object) -> None:
    lbug = base / "lbug"
    lbug.mkdir(parents=True)
    (lbug / "graph.json").write_text(json.dumps(data), encoding="utf-8")


def test_from_gitnexus_dir_list_format():
    """Parse the LadybugDB list-of-items JSON format."""
    items = [
        {
            "id": "File:src/main.py",
            "label": "File",
            "properties": {"filePath": "src/main.py"},
        },
        {
            "id": "File:src/lib.py",
            "label": "File",
            "properties": {"filePath": "src/lib.py"},
        },
        {
            "id": "rel-1",
            "sourceId": "File:src/main.py",
            "targetId": "File:src/lib.py",
            "type": "IMPORTS",
            "confidence": 0.9,
            "reason": "direct import",
        },
    ]
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        _write_lbug_dir(base, items)
        g = ModuleDependencyGraph.from_gitnexus_dir(base, target_file="src/main.py")

    assert g.get_node("File:src/main.py") is not None
    assert g.get_node("File:src/lib.py") is not None
    assert len(g.relationships_of_type("IMPORTS")) == 1
    rel = g.relationships_of_type("IMPORTS")[0]
    assert rel.confidence == 0.9
    assert rel.reason == "direct import"


def test_from_gitnexus_dir_dict_format():
    """Parse the LadybugDB dict-with-nodes/relationships JSON format."""
    data = {
        "nodes": [
            {
                "id": "File:app.py",
                "label": "File",
                "properties": {"filePath": "app.py"},
            },
            {
                "id": "File:utils.py",
                "label": "File",
                "properties": {"filePath": "utils.py"},
            },
        ],
        "relationships": [
            {
                "sourceId": "File:app.py",
                "targetId": "File:utils.py",
                "type": "IMPORTS",
            }
        ],
    }
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        _write_lbug_dir(base, data)
        g = ModuleDependencyGraph.from_gitnexus_dir(base, target_file="app.py")

    assert g.get_node("File:app.py") is not None
    assert len(g.relationships_of_type("IMPORTS")) == 1
    # Auto-generated relationship id
    rel = g.relationships_of_type("IMPORTS")[0]
    assert rel.source_id == "File:app.py"


def test_from_gitnexus_dir_missing_lbug_raises():
    """Missing lbug/ directory raises FileNotFoundError."""
    import pytest

    with (
        tempfile.TemporaryDirectory() as tmp,
        pytest.raises(FileNotFoundError, match="LadybugDB"),
    ):
        ModuleDependencyGraph.from_gitnexus_dir(tmp, target_file="x.py")


def test_from_gitnexus_dir_list_format_auto_id():
    """Relationships without an explicit id get an auto-generated id."""
    items = [
        {"id": "File:a.py", "label": "File", "properties": {"filePath": "a.py"}},
        {"id": "File:b.py", "label": "File", "properties": {"filePath": "b.py"}},
        {
            "sourceId": "File:a.py",
            "targetId": "File:b.py",
            "type": "IMPORTS",
        },
    ]
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        _write_lbug_dir(base, items)
        g = ModuleDependencyGraph.from_gitnexus_dir(base, target_file="a.py")

    assert len(g.relationships_of_type("IMPORTS")) == 1


# ---------------------------------------------------------------------------
# dep_policies — bin boundary coverage
# ---------------------------------------------------------------------------


def test_dep_policies_score_coupling_perfect():
    from topos.evaluation.policies.base import Priority
    from topos.evaluation.policies.composable import score_coupling

    # Ideal instability, low fan-in/out → high score, target achieved
    d = score_coupling(instability=0.5, fan_in=0, fan_out=0, priority=Priority.SECURE)
    assert d.score == 1.0
    assert d.achieved is True


def test_dep_policies_score_coupling_pathological():
    from topos.evaluation.policies.base import Priority
    from topos.evaluation.policies.composable import score_coupling

    # Worst instability, high fan-in/out → low score, target not achieved
    d = score_coupling(instability=1.0, fan_in=40, fan_out=40, priority=Priority.SECURE)
    assert d.score == 0.0
    assert d.achieved is False


def test_dep_policies_score_coupling_independent_thresholds():
    from topos.evaluation.policies.composable import score_coupling

    # Pass all
    assert score_coupling(instability=0.5, fan_in=10, fan_out=10).achieved is True

    # Fail instability
    assert score_coupling(instability=0.1, fan_in=10, fan_out=10).achieved is False

    # Fail fan-in
    assert score_coupling(instability=0.5, fan_in=16, fan_out=10).achieved is False

    # Fail fan-out
    assert score_coupling(instability=0.5, fan_in=10, fan_out=16).achieved is False


def test_dep_policies_score_instability_optimal_range():
    from topos.evaluation.policies.composable import _instability_tent

    # Instability in [0.3, 0.7] → quality = 1.0
    assert _instability_tent(0.5) == 1.0
    assert _instability_tent(0.3) == 1.0
    assert _instability_tent(0.7) == 1.0

    # Outside optimal range → quality < 1.0
    assert _instability_tent(0.0) == 0.0
    assert _instability_tent(1.0) == 0.0


def test_dep_policies_score_coupling_returns_scored_decision():
    from topos.evaluation.policies.base import Priority, ScoredDecision
    from topos.evaluation.policies.composable import score_coupling

    decision = score_coupling(
        instability=0.5, fan_in=3.0, fan_out=2.0, priority=Priority.SECURE
    )
    assert isinstance(decision, ScoredDecision)
    assert 0.0 <= decision.score <= 1.0
    assert "mdg.instability" in decision.interpretation
    assert "mdg.fan_in" in decision.interpretation
    assert "mdg.fan_out" in decision.interpretation


def test_score_mdg_routes_to_active_metrics():
    from topos.evaluation.characteristic_morphism import _score_composable_dim
    from topos.evaluation.policies.base import Priority

    # Extra metrics (coupling, dep_depth) are passed through raw_metrics
    # but _score_composable_dim uses instability, fan_in, fan_out
    decision = _score_composable_dim(
        {
            "mdg.coupling": 6.0,
            "mdg.instability": 0.5,
            "mdg.fan_in": 10.0,
            "mdg.fan_out": 8.0,
            "mdg.dep_depth": 3.0,
        },
        Priority.SECURE,
    )
    assert decision is not None
    assert set(decision.interpretation.keys()) == {
        "mdg.instability",
        "mdg.fan_in",
        "mdg.fan_out",
    }


# ---------------------------------------------------------------------------
# _owning_file — multi-level CONTAINS traversal
# ---------------------------------------------------------------------------


def test_owning_file_method_inside_class_inside_file():
    """Method → Class → File: _owning_file must walk both CONTAINS hops."""
    from topos.functors.probes.mdg.coupling import _owning_file

    g = ModuleDependencyGraph(target_file="mod.py")
    g.add_node(
        GraphNode(id="File:mod.py", label="File", properties={"filePath": "mod.py"})
    )
    g.add_node(GraphNode(id="Class:mod:MyClass", label="Class", properties={}))
    g.add_node(GraphNode(id="Method:mod:MyClass:run", label="Method", properties={}))
    g.add_relationship(
        GraphRelationship(
            id="c1",
            source_id="File:mod.py",
            target_id="Class:mod:MyClass",
            type="CONTAINS",
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="c2",
            source_id="Class:mod:MyClass",
            target_id="Method:mod:MyClass:run",
            type="CONTAINS",
        )
    )

    assert _owning_file(g, "Method:mod:MyClass:run") == "File:mod.py"
    assert _owning_file(g, "Class:mod:MyClass") == "File:mod.py"


def test_owning_file_contains_cycle_returns_none():
    """A CONTAINS cycle must not hang; _owning_file returns None."""
    from topos.functors.probes.mdg.coupling import _owning_file

    g = ModuleDependencyGraph(target_file="x.py")
    g.add_node(GraphNode(id="A", label="Class", properties={}))
    g.add_node(GraphNode(id="B", label="Class", properties={}))
    g.add_relationship(
        GraphRelationship(id="c1", source_id="A", target_id="B", type="CONTAINS")
    )
    g.add_relationship(
        GraphRelationship(id="c2", source_id="B", target_id="A", type="CONTAINS")
    )
    # Neither A nor B is a File, and they form a cycle
    assert _owning_file(g, "A") is None
    assert _owning_file(g, "B") is None


def test_coupling_counts_nested_method_imports():
    """Efferent coupling via a Method nested in a Class should be counted."""
    g = ModuleDependencyGraph(target_file="a.py")
    g.add_node(GraphNode(id="File:a.py", label="File", properties={"filePath": "a.py"}))
    g.add_node(GraphNode(id="Class:a:A", label="Class", properties={}))
    g.add_node(GraphNode(id="Method:a:A:go", label="Method", properties={}))
    g.add_node(GraphNode(id="File:b.py", label="File", properties={"filePath": "b.py"}))
    # File:a.py → Class:a:A → Method:a:A:go  (nested two levels)
    g.add_relationship(
        GraphRelationship(
            id="c1", source_id="File:a.py", target_id="Class:a:A", type="CONTAINS"
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="c2", source_id="Class:a:A", target_id="Method:a:A:go", type="CONTAINS"
        )
    )
    # Method:a:A:go imports File:b.py
    g.add_relationship(
        GraphRelationship(
            id="i1",
            source_id="Method:a:A:go",
            target_id="File:b.py",
            type="IMPORTS",
        )
    )

    result = calculate_coupling(g, "File:a.py")
    assert result.efferent == 1
    assert result.afferent == 0


# ---------------------------------------------------------------------------
# from_gitnexus_dir — malformed JSON
# ---------------------------------------------------------------------------


def test_from_gitnexus_dir_malformed_json():
    """A JSON file with invalid content raises json.JSONDecodeError."""
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        lbug = base / "lbug"
        lbug.mkdir()
        (lbug / "bad.json").write_text("not valid json {")

        with pytest.raises(json.JSONDecodeError):
            ModuleDependencyGraph.from_gitnexus_dir(base, target_file="foo.py")


def test_from_ladybugdb_schema_mismatch_raises_actionable_error() -> None:
    """Binary lbug with newer storage version yields LadybugSchemaMismatchError."""
    mismatch = RuntimeError(
        "Runtime exception: Trying to read a database file with a different version. "
        "Database file version: 41, Current build storage version: 40"
    )
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        lbug_file = base / "lbug"
        lbug_file.write_bytes(b"\x00")

        mock_lb = MagicMock()
        mock_lb.Database.side_effect = mismatch
        with (
            patch.dict("sys.modules", {"ladybug": mock_lb}),
            pytest.raises(LadybugSchemaMismatchError, match="ladybug 0.17\\+"),
        ):
            ModuleDependencyGraph.from_gitnexus_dir(base, target_file="foo.py")


def test_load_dep_graph_returns_none_on_schema_mismatch(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    from topos.mcp import evaluation as mcp_evaluation

    monkeypatch.setattr(
        mcp_evaluation,
        "dep_graph_for",
        MagicMock(
            side_effect=LadybugSchemaMismatchError(
                "LadybugDB storage version mismatch while loading .gitnexus/lbug."
            )
        ),
    )
    mcp_evaluation.clear_dep_graph_error()
    gitnexus_dir = tmp_path / ".gitnexus"
    gitnexus_dir.mkdir()

    result = mcp_evaluation.load_dep_graph(gitnexus_dir, "foo.py")

    assert result is None
    assert mcp_evaluation.last_dep_graph_error() is not None
    assert "storage version mismatch" in mcp_evaluation.last_dep_graph_error().lower()


# ---------------------------------------------------------------------------
# calculate_dependency_depth — diamond topology
# ---------------------------------------------------------------------------


def test_dependency_depth_diamond():
    """Diamond A→B, A→C, B→D, C→D: depth from A should be 2, D counted once."""
    g = ModuleDependencyGraph(target_file="a.py")
    for name in ("a", "b", "c", "d"):
        g.add_node(
            GraphNode(
                id=f"File:{name}.py",
                label="File",
                properties={"filePath": f"{name}.py"},
            )
        )
        g.add_node(
            GraphNode(
                id=f"Module:{name}",
                label="Module",
                properties={},
            )
        )
        g.add_relationship(
            GraphRelationship(
                id=f"c_{name}",
                source_id=f"File:{name}.py",
                target_id=f"Module:{name}",
                type="CONTAINS",
            )
        )

    for src, tgt, rid in [
        ("a", "b", "ab"),
        ("a", "c", "ac"),
        ("b", "d", "bd"),
        ("c", "d", "cd"),
    ]:
        g.add_relationship(
            GraphRelationship(
                id=rid,
                source_id=f"File:{src}.py",
                target_id=f"File:{tgt}.py",
                type="IMPORTS",
            )
        )

    depth = calculate_dependency_depth(g, "File:a.py")
    assert depth == 2


# ---------------------------------------------------------------------------
# classify_detailed — depgraph interpretation strings surfaced
# ---------------------------------------------------------------------------


def test_classify_detailed_interpretation_includes_depgraph():
    """Coupling/instability interpretation strings must appear in
    ClassificationResult."""
    from topos.core.morphism import ProgramMorphism
    from topos.evaluation.characteristic_morphism import CharacteristicMorphism

    source = "x = 1\n"
    morphism = ProgramMorphism(source=source)

    g = ModuleDependencyGraph(target_file="x.py")
    g.add_node(GraphNode(id="File:x.py", label="File", properties={"filePath": "x.py"}))

    classifier = CharacteristicMorphism()
    result = classifier.classify_detailed(morphism, representations=[g])

    assert "mdg.instability" in result.interpretation
    assert "mdg.fan_in" in result.interpretation
    assert "mdg.fan_out" in result.interpretation
    assert result.interpretation["mdg.instability"] != ""
    assert result.interpretation["mdg.fan_in"] != ""
    assert result.interpretation["mdg.fan_out"] != ""
