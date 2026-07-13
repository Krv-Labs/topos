"""Tests for depgraph-specific metrics (coupling, fan, depth)."""

import json
import tempfile
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest
from topos.functors.probes.mdg.coupling import (
    CouplingResult,
    calculate_instability_from_result,
)
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


def test_coupling_result_total():
    r = CouplingResult(afferent=3, efferent=7)
    assert r.total == 10


# ---------------------------------------------------------------------------
# Instability
# ---------------------------------------------------------------------------


def test_instability_from_precomputed_coupling():
    result = CouplingResult(afferent=3, efferent=1)
    instability = calculate_instability_from_result(result)
    assert instability == 0.25


# ---------------------------------------------------------------------------
# Dependency depth
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# Fan-in / Fan-out
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# Integration: depgraph verdicts through the classifier
# ---------------------------------------------------------------------------


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


# ---------------------------------------------------------------------------
# outgoing / incoming without rel_type filter
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# ModuleDependencyGraph.from_gitnexus_dir — list-format JSON
# ---------------------------------------------------------------------------


def _write_lbug_dir(base: Path, data: object) -> None:
    lbug = base / "lbug"
    lbug.mkdir(parents=True)
    (lbug / "graph.json").write_text(json.dumps(data), encoding="utf-8")


def test_from_gitnexus_dir_missing_lbug_raises():
    """Missing lbug/ directory raises FileNotFoundError."""
    import pytest

    with (
        tempfile.TemporaryDirectory() as tmp,
        pytest.raises(FileNotFoundError, match="LadybugDB"),
    ):
        ModuleDependencyGraph.from_gitnexus_dir(tmp, target_file="x.py")


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


# ---------------------------------------------------------------------------
# _owning_file — multi-level CONTAINS traversal
# ---------------------------------------------------------------------------


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


def _empty_query_result() -> MagicMock:
    result = MagicMock()
    result.has_next.return_value = False
    return result


def test_from_ladybugdb_retries_read_write_on_shadow_replay() -> None:
    """Shadow pages pending replay (issue #136) retry with a read-write handle."""
    shadow_error = RuntimeError(
        "Runtime exception: Couldn't replay shadow pages under read-only mode. "
        "Please re-open the database with read-write mode to replay shadow pages."
    )
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        lbug_file = base / "lbug"
        lbug_file.write_bytes(b"\x00")

        mock_lb = MagicMock()
        mock_db = MagicMock()
        mock_lb.Database.side_effect = [shadow_error, mock_db]
        mock_lb.Connection.return_value.execute.return_value = _empty_query_result()

        with patch.dict("sys.modules", {"ladybug": mock_lb}):
            graph = ModuleDependencyGraph.from_gitnexus_dir(base, target_file="foo.py")

        assert isinstance(graph, ModuleDependencyGraph)
        assert mock_lb.Database.call_count == 2
        first_call, second_call = mock_lb.Database.call_args_list
        assert first_call.kwargs["read_only"] is True
        assert second_call.kwargs["read_only"] is False


def test_from_ladybugdb_shadow_replay_retry_failure_still_raises() -> None:
    """If the read-write retry itself fails for an unrelated reason, propagate it."""
    shadow_error = RuntimeError(
        "Runtime exception: Couldn't replay shadow pages under read-only mode. "
        "Please re-open the database with read-write mode to replay shadow pages."
    )
    permission_error = RuntimeError("Runtime exception: Permission denied")
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        lbug_file = base / "lbug"
        lbug_file.write_bytes(b"\x00")

        mock_lb = MagicMock()
        mock_lb.Database.side_effect = [shadow_error, permission_error]

        with (
            patch.dict("sys.modules", {"ladybug": mock_lb}),
            pytest.raises(RuntimeError, match="Permission denied"),
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


def test_load_dep_graph_degrades_gracefully_on_unrecognized_runtime_error(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """A Ladybug RuntimeError with an unrecognized message must not crash the caller.

    Regression test for issue #136: previously only "different version" /
    "storage version" messages were tolerated, so any other Ladybug failure
    (e.g. a corrupted WAL, or a failed shadow-page-replay retry) propagated
    as an unhandled exception through the CLI and MCP tool surfaces.
    """
    from topos.mcp import evaluation as mcp_evaluation

    monkeypatch.setattr(
        mcp_evaluation,
        "dep_graph_for",
        MagicMock(
            side_effect=RuntimeError(
                "Runtime exception: Corrupted wal file. "
                "Read out invalid WAL record type."
            )
        ),
    )
    mcp_evaluation.clear_dep_graph_error()
    gitnexus_dir = tmp_path / ".gitnexus"
    gitnexus_dir.mkdir()

    result = mcp_evaluation.load_dep_graph(gitnexus_dir, "foo.py")

    assert result is None
    assert mcp_evaluation.last_dep_graph_error() is not None
    assert "corrupted wal file" in mcp_evaluation.last_dep_graph_error().lower()


# ---------------------------------------------------------------------------
# calculate_dependency_depth — diamond topology
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# classify_detailed — depgraph interpretation strings surfaced
# ---------------------------------------------------------------------------
