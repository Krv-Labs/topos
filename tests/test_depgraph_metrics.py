"""Tests for depgraph-specific metrics (coupling, fan, depth)."""

from topos.metrics.depgraph.coupling import (
    CouplingResult,
    calculate_coupling,
    calculate_dependency_depth,
    calculate_instability,
)
from topos.metrics.depgraph.fan import FanResult, calculate_fan_in_out
from topos.representations.depgraph.graph import (
    DependencyGraph,
    GraphNode,
    GraphRelationship,
)


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

    assert result.is_valid
    assert isinstance(result.evaluation, EvaluationValue)
    assert "depgraph" in result.representation_metrics
    assert "depgraph.coupling" in result.representation_metrics["depgraph"]


def test_classifier_without_representations_unchanged():
    from topos.core.morphism import ProgramMorphism
    from topos.logic.omega import SubobjectClassifier

    source = "x = 1"
    morphism = ProgramMorphism(source=source)
    classifier = SubobjectClassifier()

    result_without = classifier.classify_detailed(morphism)
    result_with_none = classifier.classify_detailed(morphism, representations=None)
    result_with_empty = classifier.classify_detailed(morphism, representations=[])

    assert result_without.evaluation == result_with_none.evaluation
    assert result_without.evaluation == result_with_empty.evaluation
    assert result_without.representation_metrics == {}


def test_classification_result_str_with_representations():
    from topos.logic.lattice import EvaluationValue
    from topos.logic.omega import ClassificationResult

    result = ClassificationResult(
        evaluation=EvaluationValue.COMMODITY,
        complexity_score=0.5,
        entropy_score=0.4,
        is_valid=True,
        representation_metrics={
            "depgraph": {
                "depgraph.coupling": 3.0,
                "depgraph.instability": 0.5,
            }
        },
    )
    text = str(result)
    assert "depgraph" in text
    assert "coupling" in text
