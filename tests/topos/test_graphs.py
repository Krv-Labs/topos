"""Tests for the graphs package."""

from topos.core.object import ProgramObject
from topos.graphs.ast.object import ASTRepresentation
from topos.graphs.base import Representation
from topos.graphs.mdg.object import (
    DependencyGraph,
    GraphNode,
    GraphRelationship,
)
from topos.utils.tree_sitter import parse_python

# ---------------------------------------------------------------------------
# Protocol conformance
# ---------------------------------------------------------------------------


def test_ast_representation_conforms_to_protocol():
    source = "x = 1"
    root = parse_python(source)
    obj = ProgramObject(root=root, source=source)
    rep = ASTRepresentation(program_object=obj, source=source)
    assert isinstance(rep, Representation)


def test_dependency_graph_conforms_to_protocol():
    graph = DependencyGraph(target_file="foo.py")
    assert isinstance(graph, Representation)


# ---------------------------------------------------------------------------
# ASTRepresentation
# ---------------------------------------------------------------------------


def test_ast_representation_name():
    source = "x = 1"
    root = parse_python(source)
    obj = ProgramObject(root=root, source=source)
    rep = ASTRepresentation(program_object=obj, source=source)
    assert rep.name == "ast"


def test_ast_representation_metrics():
    source = "def foo():\n    if True:\n        pass\n    return 1"
    root = parse_python(source)
    obj = ProgramObject(root=root, source=source)
    rep = ASTRepresentation(program_object=obj, source=source)

    m = rep.metrics()
    assert "ast.complexity" in m
    assert "ast.entropy" in m
    assert m["ast.complexity"] >= 1.0
    assert 0.0 <= m["ast.entropy"] <= 2.0


# ---------------------------------------------------------------------------
# DependencyGraph construction and lookups
# ---------------------------------------------------------------------------


def _make_simple_graph() -> DependencyGraph:
    """Build a small in-memory dependency graph for testing."""
    g = DependencyGraph(target_file="src/app.py")

    g.add_node(
        GraphNode(
            id="File:src/app.py",
            label="File",
            properties={"filePath": "src/app.py", "name": "app.py"},
        )
    )
    g.add_node(
        GraphNode(
            id="File:src/utils.py",
            label="File",
            properties={"filePath": "src/utils.py", "name": "utils.py"},
        )
    )
    g.add_node(
        GraphNode(
            id="File:src/db.py",
            label="File",
            properties={"filePath": "src/db.py", "name": "db.py"},
        )
    )
    g.add_node(
        GraphNode(
            id="File:src/models.py",
            label="File",
            properties={"filePath": "src/models.py", "name": "models.py"},
        )
    )

    g.add_node(
        GraphNode(
            id="Func:app:main",
            label="Function",
            properties={"filePath": "src/app.py", "name": "main"},
        )
    )
    g.add_node(
        GraphNode(
            id="Func:utils:helper",
            label="Function",
            properties={"filePath": "src/utils.py", "name": "helper"},
        )
    )
    g.add_node(
        GraphNode(
            id="Func:db:query",
            label="Function",
            properties={"filePath": "src/db.py", "name": "query"},
        )
    )

    # CONTAINS edges
    g.add_relationship(
        GraphRelationship(
            id="c1",
            source_id="File:src/app.py",
            target_id="Func:app:main",
            type="CONTAINS",
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="c2",
            source_id="File:src/utils.py",
            target_id="Func:utils:helper",
            type="CONTAINS",
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="c3",
            source_id="File:src/db.py",
            target_id="Func:db:query",
            type="CONTAINS",
        )
    )

    # app.py imports utils.py and db.py
    g.add_relationship(
        GraphRelationship(
            id="i1",
            source_id="File:src/app.py",
            target_id="File:src/utils.py",
            type="IMPORTS",
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="i2",
            source_id="File:src/app.py",
            target_id="File:src/db.py",
            type="IMPORTS",
        )
    )

    # db.py imports models.py (transitive chain: app -> db -> models)
    g.add_relationship(
        GraphRelationship(
            id="i3",
            source_id="File:src/db.py",
            target_id="File:src/models.py",
            type="IMPORTS",
        )
    )

    # utils.py imports app.py (creates afferent coupling for app.py)
    g.add_relationship(
        GraphRelationship(
            id="i4",
            source_id="File:src/utils.py",
            target_id="File:src/app.py",
            type="IMPORTS",
        )
    )

    # CALLS edges
    g.add_relationship(
        GraphRelationship(
            id="call1",
            source_id="Func:app:main",
            target_id="Func:utils:helper",
            type="CALLS",
        )
    )
    g.add_relationship(
        GraphRelationship(
            id="call2",
            source_id="Func:app:main",
            target_id="Func:db:query",
            type="CALLS",
        )
    )

    return g


def test_depgraph_name():
    g = DependencyGraph(target_file="foo.py")
    assert g.name == "depgraph"


def test_depgraph_node_lookups():
    g = _make_simple_graph()
    assert g.get_node("File:src/app.py") is not None
    assert g.get_node("nonexistent") is None
    assert len(g.nodes_of_label("File")) == 4
    assert len(g.nodes_of_label("Function")) == 3


def test_depgraph_relationship_lookups():
    g = _make_simple_graph()
    assert len(g.relationships_of_type("IMPORTS")) == 4
    assert len(g.relationships_of_type("CALLS")) == 2
    assert len(g.relationships_of_type("CONTAINS")) == 3


def test_depgraph_outgoing_incoming():
    g = _make_simple_graph()
    outgoing_imports = g.outgoing("File:src/app.py", "IMPORTS")
    assert len(outgoing_imports) == 2

    incoming_imports = g.incoming("File:src/app.py", "IMPORTS")
    assert len(incoming_imports) == 1


def test_depgraph_file_node_id():
    g = _make_simple_graph()
    assert g.file_node_id() == "File:src/app.py"

    g2 = DependencyGraph(target_file="nonexistent.py")
    assert g2.file_node_id() is None


def test_depgraph_contained_symbols():
    g = _make_simple_graph()
    symbols = g.contained_symbols("File:src/app.py")
    assert "Func:app:main" in symbols


def test_depgraph_metrics():
    g = _make_simple_graph()
    m = g.metrics()
    assert "depgraph.coupling" in m
    assert "depgraph.instability" in m
    assert "depgraph.fan_in" in m
    assert "depgraph.fan_out" in m
    assert "depgraph.dep_depth" in m
    assert m["depgraph.coupling"] > 0
    assert 0.0 <= m["depgraph.instability"] <= 1.0


def test_depgraph_metrics_no_file_found():
    g = DependencyGraph(target_file="nonexistent.py")
    m = g.metrics()
    assert m["depgraph.coupling"] == 0.0
    assert m["depgraph.instability"] == 0.5


# ---------------------------------------------------------------------------
# ASTRepresentation — verdict dispatch via registry
# ---------------------------------------------------------------------------


def test_ast_representation_dispatches_verdicts():
    """ASTRepresentation in representations must produce verdicts via the registry."""
    from topos.core.morphism import ProgramMorphism
    from topos.core.object import ProgramObject
    from topos.logic.omega import SubobjectClassifier
    from topos.utils.tree_sitter import parse_python

    source = "def foo():\n    if True:\n        pass\n    return 1\n"
    morphism = ProgramMorphism(source=source)
    root = parse_python(source)
    obj = ProgramObject(root=root, source=source)
    ast_rep = ASTRepresentation(program_object=obj, source=source)

    classifier = SubobjectClassifier()
    result = classifier.classify_detailed(morphism, representations=[ast_rep])

    # Metrics must be stored in raw_metrics
    assert "ast.complexity" in result.raw_metrics
    assert "ast.entropy" in result.raw_metrics

    # Result must equal baseline (same data path; meet is idempotent)
    baseline = classifier.classify_detailed(morphism)
    assert result.summary() == baseline.summary()


def test_score_ast_produces_scored_decision():
    """_score_ast returns a ScoredDecision for the legacy ast.entropy path
    (folding into the SIMPLE generator).
    """
    from topos.logic.omega import _score_ast
    from topos.logic.policies.base import Priority, ScoredDecision

    decision = _score_ast(
        {"ast.complexity": 2.0, "ast.entropy": 0.5}, Priority.BALANCED
    )
    assert decision is not None
    assert isinstance(decision, ScoredDecision)
    assert 0.0 <= decision.score <= 1.0
    # The legacy AST dispatcher feeds into Φ_SIMPLE which names its
    # principal complexity metric `cfg.cyclomatic`.  Entropy is still in
    # the interpretation map.
    keys = set(decision.interpretation.keys())
    assert "ast.entropy" in keys
    assert all(v != "" for v in decision.interpretation.values())


def test_score_ast_returns_none_on_missing_keys():
    """_score_ast returns None when expected metric keys are absent."""
    from topos.logic.omega import _score_ast
    from topos.logic.policies.base import Priority

    assert _score_ast({}, Priority.BALANCED) is None
    assert _score_ast({"depgraph.coupling": 3.0}, Priority.BALANCED) is None


# ---------------------------------------------------------------------------
# Backward compatibility of metric imports
# ---------------------------------------------------------------------------


def test_backward_compat_top_level_imports():
    from topos import (
        ASTRepresentation,
        DependencyGraph,
        Representation,
    )

    assert ASTRepresentation is not None
    assert DependencyGraph is not None
    assert Representation is not None


# ---------------------------------------------------------------------------
# file_node_id — path-matching branch coverage
# ---------------------------------------------------------------------------


def _graph_with_file(target_file: str, file_path_property: str) -> DependencyGraph:
    """Minimal graph with one File node for path-matching tests."""
    g = DependencyGraph(target_file=target_file)
    g.add_node(
        GraphNode(
            id="File:node",
            label="File",
            properties={"filePath": file_path_property},
        )
    )
    return g


def test_file_node_id_exact_match():
    """Exact equality between target_file and filePath."""
    g = _graph_with_file("src/foo.py", "src/foo.py")
    assert g.file_node_id() == "File:node"


def test_file_node_id_suffix_match():
    """filePath ends with '/<target_file>'."""
    g = _graph_with_file("foo.py", "src/foo.py")
    assert g.file_node_id() == "File:node"


def test_file_node_id_reverse_suffix_match():
    """target_file ends with '/<filePath>'."""
    g = _graph_with_file("src/foo.py", "foo.py")
    assert g.file_node_id() == "File:node"


def test_file_node_id_no_match():
    """Neither path matches — returns None."""
    g = _graph_with_file("bar.py", "foo.py")
    assert g.file_node_id() is None
