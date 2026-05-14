"""Tests for depgraph-specific metrics (coupling, fan, depth)."""

import json
import tempfile
from pathlib import Path

from topos.graphs.mdg.object import (
    DependencyGraph,
    GraphNode,
    GraphRelationship,
)
from topos.functors.probes.mdg.coupling import (
    CouplingResult,
    calculate_coupling,
    calculate_dependency_depth,
    calculate_instability,
    calculate_instability_from_result,
)
from topos.functors.probes.mdg.fan import FanResult, calculate_fan_in_out


def _graph_with_linear_chain() -> DependencyGraph:
    """A -> B -> C -> D linear import chain, target = A."""
    g = DependencyGraph(target_file="a.py")
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


def _graph_with_fan() -> DependencyGraph:
    """Hub file with many callers and callees."""
    g = DependencyGraph(target_file="hub.py")
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
    g = DependencyGraph(target_file="isolated.py")
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
    g = DependencyGraph(target_file="lone.py")
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
    g = DependencyGraph(target_file="x.py")
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
    g = DependencyGraph(target_file="solo.py")
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
    from topos.logic.lattice import EvaluationValue
    from topos.logic.omega import SubobjectClassifier

    source = "def main(): return 1"
    morphism = ProgramMorphism(source=source)
    classifier = SubobjectClassifier()

    g = _graph_with_linear_chain()
    result = classifier.classify_detailed(morphism, representations=[g])

    assert result.is_parseable
    assert isinstance(result.summary(), EvaluationValue)
    assert "composable" in result.dimensions
    assert "depgraph.coupling" in result.raw_metrics


def test_classifier_without_representations_unchanged():
    from topos.core.morphism import ProgramMorphism
    from topos.logic.omega import SubobjectClassifier

    source = "x = 1"
    morphism = ProgramMorphism(source=source)
    classifier = SubobjectClassifier()

    result_without = classifier.classify_detailed(morphism)
    result_with_none = classifier.classify_detailed(morphism, representations=None)
    result_with_empty = classifier.classify_detailed(morphism, representations=[])

    assert result_without.summary() == result_with_none.summary()
    assert result_without.summary() == result_with_empty.summary()
    # Without extra representations, the AST representation alone feeds
    # the SIMPLE generator.
    assert set(result_without.dimensions.keys()) == {"simple"}


def test_classification_result_str_with_representations():
    from topos.logic.lattice import EvaluationValue
    from topos.logic.omega import ClassificationResult

    result = ClassificationResult(
        is_parseable=True,
        dimensions={"coupling": EvaluationValue.COMPOSABLE},
        scores={"coupling": 0.75},
        lattice_element=EvaluationValue.COMPOSABLE,
        raw_metrics={
            "depgraph.coupling": 3.0,
            "depgraph.instability": 0.5,
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

    g = DependencyGraph(target_file="orphan.py")
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

    g = DependencyGraph(target_file="x.py")
    assert _owning_file(g, "nonexistent-id") is None


def test_owning_file_via_contains_edge():
    """A Function reachable via a CONTAINS edge resolves to its File owner."""
    from topos.functors.probes.mdg.coupling import _owning_file

    g = DependencyGraph(target_file="owner.py")
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
    g = DependencyGraph(target_file="empty.py")
    assert g.outgoing("no-such-node") == []
    assert g.incoming("no-such-node") == []


# ---------------------------------------------------------------------------
# DependencyGraph.from_gitnexus_dir — list-format JSON
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
        g = DependencyGraph.from_gitnexus_dir(base, target_file="src/main.py")

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
        g = DependencyGraph.from_gitnexus_dir(base, target_file="app.py")

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
        DependencyGraph.from_gitnexus_dir(tmp, target_file="x.py")


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
        g = DependencyGraph.from_gitnexus_dir(base, target_file="a.py")

    assert len(g.relationships_of_type("IMPORTS")) == 1


# ---------------------------------------------------------------------------
# dep_policies — bin boundary coverage
# ---------------------------------------------------------------------------


def test_dep_policies_score_coupling_perfect():
    from topos.logic.policies.base import Priority
    from topos.logic.policies.coupling import score_coupling

    # Low coupling, ideal instability → high score, target achieved
    d = score_coupling(0.0, 0.5, Priority.BALANCED)
    assert d.score >= 0.9
    assert d.achieved is True


def test_dep_policies_score_coupling_pathological():
    from topos.logic.policies.base import Priority
    from topos.logic.policies.coupling import score_coupling

    # Max coupling, worst instability → low score, target not achieved
    d = score_coupling(35.0, 1.0, Priority.BALANCED)
    assert d.score < 0.6
    assert d.achieved is False


def test_dep_policies_score_coupling_priority_shifts_weight():
    from topos.logic.policies.base import Priority
    from topos.logic.policies.coupling import score_coupling

    # High coupling (bad), perfect instability (good)
    balanced = score_coupling(20.0, 0.5, Priority.BALANCED)
    composable = score_coupling(20.0, 0.5, Priority.COMPOSABLE)
    # COMPOSABLE upweights coupling_quality (which is bad here) → lower score
    assert composable.score <= balanced.score


def test_dep_policies_score_instability_optimal_range():
    from topos.logic.policies.coupling import _instability_tent

    # Instability in [0.3, 0.7] → quality = 1.0
    assert _instability_tent(0.5) == 1.0
    assert _instability_tent(0.3) == 1.0
    assert _instability_tent(0.7) == 1.0

    # Outside optimal range → quality < 1.0
    assert _instability_tent(0.0) < 1.0
    assert _instability_tent(1.0) == 0.0


def test_dep_policies_score_coupling_returns_scored_decision():
    from topos.logic.policies.base import Priority, ScoredDecision
    from topos.logic.policies.coupling import score_coupling

    decision = score_coupling(3.0, 0.5, Priority.BALANCED)
    assert isinstance(decision, ScoredDecision)
    assert 0.0 <= decision.score <= 1.0
    assert "depgraph.coupling" in decision.interpretation
    assert "depgraph.instability" in decision.interpretation


def test_score_depgraph_routes_to_active_metrics():
    from topos.logic.omega import _score_depgraph
    from topos.logic.policies.base import Priority

    # Extra metrics (fan_in, fan_out, dep_depth) are passed through raw_metrics
    # but _score_depgraph only uses coupling and instability
    decision = _score_depgraph(
        {
            "depgraph.coupling": 6.0,
            "depgraph.instability": 0.5,
            "depgraph.fan_in": 10.0,
            "depgraph.fan_out": 8.0,
            "depgraph.dep_depth": 3.0,
        },
        Priority.BALANCED,
    )
    assert decision is not None
    assert set(decision.interpretation.keys()) == {
        "depgraph.coupling",
        "depgraph.instability",
    }


# ---------------------------------------------------------------------------
# _owning_file — multi-level CONTAINS traversal
# ---------------------------------------------------------------------------


def test_owning_file_method_inside_class_inside_file():
    """Method → Class → File: _owning_file must walk both CONTAINS hops."""
    from topos.functors.probes.mdg.coupling import _owning_file

    g = DependencyGraph(target_file="mod.py")
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

    g = DependencyGraph(target_file="x.py")
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
    g = DependencyGraph(target_file="a.py")
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
    import pytest

    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        lbug = base / "lbug"
        lbug.mkdir()
        (lbug / "bad.json").write_text("not valid json {")

        with pytest.raises(json.JSONDecodeError):
            DependencyGraph.from_gitnexus_dir(base, target_file="foo.py")


# ---------------------------------------------------------------------------
# calculate_dependency_depth — diamond topology
# ---------------------------------------------------------------------------


def test_dependency_depth_diamond():
    """Diamond A→B, A→C, B→D, C→D: depth from A should be 2, D counted once."""
    g = DependencyGraph(target_file="a.py")
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
    from topos.logic.omega import SubobjectClassifier

    source = "x = 1\n"
    morphism = ProgramMorphism(source=source)

    g = DependencyGraph(target_file="x.py")
    g.add_node(GraphNode(id="File:x.py", label="File", properties={"filePath": "x.py"}))

    classifier = SubobjectClassifier()
    result = classifier.classify_detailed(morphism, representations=[g])

    assert "depgraph.coupling" in result.interpretation
    assert "depgraph.instability" in result.interpretation
    assert result.interpretation["depgraph.coupling"] != ""
    assert result.interpretation["depgraph.instability"] != ""
