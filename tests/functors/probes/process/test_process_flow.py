"""Tests for the GitNexus process-flow representation and its probes."""

from __future__ import annotations

from topos.evaluation.characteristic_morphism import (
    _score_secure_dim,
    _score_simple_dim,
)
from topos.evaluation.policies.base import Priority
from topos.graphs.mdg.object import GraphNode, GraphRelationship, ModuleDependencyGraph
from topos.graphs.process.object import ProcessFlow, ProcessFlowGraph


def _graph_with_process(
    *,
    process_type: str = "cross_community",
    step_count: int = 4,
    communities: list[str] | None = None,
    step_symbol_ids: tuple[str, ...] = (
        "Function:app.py:handler",
        "Function:lib/util.py:helper",
    ),
) -> ModuleDependencyGraph:
    """An MDG holding a single Process whose steps touch ``app.py``."""
    g = ModuleDependencyGraph(target_file="app.py")
    g.add_node(
        GraphNode(
            id="proc_0_handler",
            label="Process",
            properties={
                "id": "proc_0_handler",
                "label": "Handler -> helper",
                "processType": process_type,
                "stepCount": step_count,
                "communities": communities
                if communities is not None
                else ["'comm_1'", "'comm_2'", "'comm_3'"],
                "entryPointId": step_symbol_ids[0],
                "terminalId": step_symbol_ids[-1],
            },
        )
    )
    for sid in step_symbol_ids:
        g.add_relationship(
            GraphRelationship(
                id=f"{sid}->proc_0_handler",
                source_id=sid,
                target_id="proc_0_handler",
                type="STEP_IN_PROCESS",
            )
        )
    return g


def test_from_dep_graph_keeps_flows_touching_file():
    g = _graph_with_process()
    pfg = ProcessFlowGraph.from_dep_graph(g, "app.py")
    assert len(pfg.flows) == 1
    flow = pfg.flows[0]
    assert flow.id == "proc_0_handler"
    assert flow.step_count == 4
    assert flow.process_type == "cross_community"
    # Community ids are stripped of GitNexus's stray quotes.
    assert flow.communities == ("comm_1", "comm_2", "comm_3")


def test_flows_not_touching_file_are_dropped():
    g = _graph_with_process(
        step_symbol_ids=("Function:other.py:a", "Function:other.py:b")
    )
    pfg = ProcessFlowGraph.from_dep_graph(g, "app.py")
    assert pfg.flows == ()


def test_metrics_by_dimension():
    g = _graph_with_process(step_count=9)
    pfg = ProcessFlowGraph.from_dep_graph(g, "app.py", dimension="simple")
    assert pfg.metrics() == {
        "process.max_flow_length": 9.0,
        "process.flow_participation": 1.0,
    }
    composable = pfg.for_dimension("composable")
    assert composable.metrics() == {
        "process.max_community_span": 3.0,
        "process.cross_community_flows": 1.0,
    }
    secure = pfg.for_dimension("secure")
    assert secure.metrics() == {"process.dangerous_flows": 0.0}


def test_secure_detects_dangerous_step_on_flow():
    g = _graph_with_process(
        step_symbol_ids=("Function:app.py:handler", "Function:lib/run.py:eval")
    )
    pfg = ProcessFlowGraph.from_dep_graph(g, "app.py", dimension="secure")
    assert pfg.metrics() == {"process.dangerous_flows": 1.0}


def test_step_names_include_entry_and_terminal():
    flow = ProcessFlow(
        id="p",
        label="x",
        process_type="cross_community",
        step_count=2,
        communities=(),
        entry_point_id="Function:a.py:start",
        terminal_id="Function:b.py:finish",
        step_symbol_ids=("Function:a.py:start", "Function:b.py:finish"),
    )
    assert set(flow.step_names()) == {"start", "finish"}


def test_dispatcher_merges_process_into_secure_gate():
    # A dangerous flow must flip the SECURE generator off even when the
    # intra-file CPG metrics are clean.
    raw = {
        "cpg.dangerous_calls": 0.0,
        "cpg.taint_flows": 0.0,
        "process.dangerous_flows": 1.0,
    }
    decision = _score_secure_dim(raw, Priority.SECURE)
    assert decision is not None
    assert decision.achieved is False


def test_dispatcher_merges_process_into_simple_gate():
    # Clean CFG/AST but an over-long interprocedural flow fails SIMPLE.
    raw = {
        "cfg.cyclomatic": 1.0,
        "ast.entropy": 0.5,
        "ast.max_function_complexity": 1.0,
        "process.max_flow_length": 999.0,
    }
    decision = _score_simple_dim(raw, Priority.SECURE)
    assert decision is not None
    assert decision.achieved is False


def test_dispatcher_unaffected_without_process_metrics():
    raw = {
        "cpg.dangerous_calls": 0.0,
        "cpg.taint_flows": 0.0,
    }
    decision = _score_secure_dim(raw, Priority.SECURE)
    assert decision is not None
    assert decision.achieved is True
