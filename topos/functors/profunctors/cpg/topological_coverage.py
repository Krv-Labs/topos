"""
Topological Semantic Test Coverage via Euler Characteristic Transform (ECT).

Evaluates coverage by comparing the V - E ECT of the "test scope" static
subgraph of the program under test (PUT) against the ECT of the test execution
graph.

Node texts are embedded with fastembed (384-D), reduced to 2-D via PCA fit
jointly on PUT ∪ test, and fed into TRAILED's Rust ECT core through
``trailed.tabular.compute_ect_from_numpy(..., edge_index=...)``. Both graphs
are computed in one batched call so they share the same directions and
filtration grid.
"""

from __future__ import annotations

import importlib.util
import re
from dataclasses import dataclass

import numpy as np
from topos.graphs.cpg.models import CPGNode
from topos.graphs.cpg.object import CodePropertyGraph

_DECL_KINDS = frozenset({"FunctionDecl", "MethodDecl"})
_CALL_PREFIX = re.compile(r"^([A-Za-z_][A-Za-z0-9_.]*)\s*\(")

_EMBEDDING_MODEL = None
_ECT_ZERO_DISTANCE_TOLERANCE = 1e-8

ECT_COVERAGE_INSTALL_HINT = (
    "pip install 'topos-mcp[ect-coverage]' or use the topos-ect release binary"
)


class ECTCoverageUnavailableError(ImportError):
    """Raised when fastembed or trailed are not installed."""


def ect_coverage_available() -> bool:
    """Return True when optional ECT coverage dependencies are importable."""
    return (
        importlib.util.find_spec("fastembed") is not None
        and importlib.util.find_spec("trailed") is not None
    )


def require_ect_coverage() -> None:
    """Raise :class:`ECTCoverageUnavailableError` if optional deps are missing."""
    if ect_coverage_available():
        return
    raise ECTCoverageUnavailableError(
        "Topological (ECT) coverage requires the optional ect-coverage extra. "
        f"Install with: {ECT_COVERAGE_INSTALL_HINT}"
    )


def get_embedding_model():
    """Lazily load and reuse the fastembed TextEmbedding model."""
    require_ect_coverage()
    global _EMBEDDING_MODEL
    if _EMBEDDING_MODEL is None:
        from fastembed import TextEmbedding

        _EMBEDDING_MODEL = TextEmbedding(
            model_name="snowflake/snowflake-arctic-embed-xs"
        )
    return _EMBEDDING_MODEL


@dataclass(frozen=True)
class TopologicalCoverageReport:
    """Detailed report for topological semantic test coverage."""

    topological_distance: float
    topological_coverage_score: float
    tested_functions: tuple[str, ...]
    untested_functions: tuple[str, ...]
    put_node_count: int
    test_node_count: int
    scoped_node_count: int


def _callee_from_text(text: str) -> str:
    """Extract the dotted callee prefix from a call expression's text."""
    match = _CALL_PREFIX.match(text.strip())
    return match.group(1) if match else ""


def _decl_name(node: CPGNode, cpg: CodePropertyGraph) -> str:
    """Extract the name of a FunctionDecl or MethodDecl node."""
    name = node.attributes.get("name")
    if name:
        return name
    for child in node.uast.children:
        if child.kind == "Identifier":
            span = child.span
            if cpg.source and span.end_byte <= len(cpg.source.encode("utf-8")):
                try:
                    return cpg.source.encode("utf-8")[
                        span.start_byte : span.end_byte
                    ].decode("utf-8")
                except Exception:
                    pass
    return node.attributes.get("scope") or "anonymous"


def get_ast_subtree_nodes(start_node: CPGNode, cpg: CodePropertyGraph) -> list[CPGNode]:
    """Recursively get all CPG nodes in the AST subtree of a given node."""
    subtree = []
    stack = [start_node]
    visited = set()
    while stack:
        curr = stack.pop()
        if curr.id in visited:
            continue
        visited.add(curr.id)
        subtree.append(curr)
        for child in curr.uast.children:
            child_id = child.id or f"anon::{id(child):x}"
            if child_id in cpg.nodes:
                stack.append(cpg.nodes[child_id])
    return subtree


def get_test_scoped_subgraph(
    put_cpg: CodePropertyGraph,
    test_cpg: CodePropertyGraph,
) -> tuple[list[CPGNode], list[str], list[str]]:
    """Subset the static CPG of the PUT based on Call Graph Reachability."""
    put_decls = [n for n in put_cpg.nodes.values() if n.kind in _DECL_KINDS]
    name_to_decl: dict[str, CPGNode] = {}
    for decl in put_decls:
        name = _decl_name(decl, put_cpg)
        if name:
            name_to_decl[name] = decl

    entry_names: set[str] = set()
    for node in test_cpg.nodes.values():
        if node.kind == "CallExpr":
            text = test_cpg.node_text(node)
            if text:
                callee = _callee_from_text(text)
                if callee in name_to_decl:
                    entry_names.add(callee)
                else:
                    short_callee = callee.split(".")[-1]
                    if short_callee in name_to_decl:
                        entry_names.add(short_callee)

    is_fallback = False
    if not entry_names and name_to_decl:
        entry_names = set(name_to_decl.keys())
        is_fallback = True

    reachable_names = set(entry_names)
    queue = list(entry_names)
    while queue:
        current_name = queue.pop(0)
        current_decl = name_to_decl.get(current_name)
        if not current_decl:
            continue

        subtree_nodes = get_ast_subtree_nodes(current_decl, put_cpg)
        for node in subtree_nodes:
            if node.kind == "CallExpr":
                text = put_cpg.node_text(node)
                if text:
                    callee = _callee_from_text(text)
                    if callee in name_to_decl and callee not in reachable_names:
                        reachable_names.add(callee)
                        queue.append(callee)
                    else:
                        short_callee = callee.split(".")[-1]
                        if (
                            short_callee in name_to_decl
                            and short_callee not in reachable_names
                        ):
                            reachable_names.add(short_callee)
                            queue.append(short_callee)

    scoped_nodes_dict: dict[str, CPGNode] = {}
    for r_name in reachable_names:
        decl = name_to_decl.get(r_name)
        if decl:
            for node in get_ast_subtree_nodes(decl, put_cpg):
                scoped_nodes_dict[node.id] = node

    if not scoped_nodes_dict:
        scoped_nodes = list(put_cpg.nodes.values())
    else:
        scoped_nodes = list(scoped_nodes_dict.values())

    tested = [] if is_fallback else list(reachable_names)
    untested = (
        [name for name in name_to_decl if name not in reachable_names]
        if not is_fallback
        else list(name_to_decl.keys())
    )

    return scoped_nodes, sorted(tested), sorted(untested)


def _embed_nodes(
    cpg: CodePropertyGraph, nodes: list[CPGNode], text_to_emb: dict[str, np.ndarray]
) -> np.ndarray:
    """Stack embeddings for ``nodes`` in order, using a shared text cache."""
    embeddings = []
    for node in nodes:
        text = cpg.node_text(node).strip() or f"{node.kind} {str(node.attributes)}"
        embeddings.append(text_to_emb[text])
    return np.asarray(embeddings, dtype=np.float32)


def _joint_pca_2d(
    emb_put: np.ndarray, emb_test: np.ndarray
) -> tuple[np.ndarray, np.ndarray]:
    """Fit a 2-component PCA jointly on PUT∪test and project both."""
    stacked = np.vstack([emb_put, emb_test])
    centered = stacked - stacked.mean(axis=0, keepdims=True)
    # Top-2 right singular vectors give the PCA loading matrix.
    _, _, vt = np.linalg.svd(centered, full_matrices=False)
    components = vt[:2]  # shape (2, d)
    projected = centered @ components.T  # shape (N_total, 2)
    n_put = emb_put.shape[0]
    return projected[:n_put].astype(np.float32), projected[n_put:].astype(np.float32)


def _local_edge_index(edges, node_id_to_local: dict[str, int]) -> np.ndarray:
    """Build a (2, E) int64 edge_index over nodes present in ``node_id_to_local``."""
    cols = []
    for e in edges:
        u = node_id_to_local.get(e.source)
        v = node_id_to_local.get(e.target)
        if u is not None and v is not None:
            cols.append((u, v))
    if not cols:
        return np.zeros((2, 0), dtype=np.int64)
    arr = np.asarray(cols, dtype=np.int64).T
    return np.ascontiguousarray(arr)


def calculate_topological_coverage(
    put_cpg: CodePropertyGraph,
    test_cpg: CodePropertyGraph,
    *,
    num_directions: int = 32,
    num_steps: int = 64,
) -> TopologicalCoverageReport:
    """Calculate the Topological Semantic Test Coverage between PUT and Test CPGs.

    1. Subset PUT CPG to define the "test scope" via Call Graph Reachability.
    2. Embed scoped PUT and test node texts via fastembed.
    3. Jointly PCA-reduce to 2-D so both graphs live in the same low-dim frame.
    4. Compute V - E ECT for both graphs in one batched TRAILED call.
    5. Normalize, take RMSE, and exponentiate into a [0, 1] score.
    """
    require_ect_coverage()
    from trailed.tabular import compute_ect_from_numpy

    scoped_nodes, tested_funcs, untested_functions = get_test_scoped_subgraph(
        put_cpg, test_cpg
    )
    test_nodes = list(test_cpg.nodes.values())

    # File nodes carry no useful semantic shape and would dominate the
    # filtration on identical sources.
    scoped_nodes = [n for n in scoped_nodes if n.kind != "File"]
    test_nodes = [n for n in test_nodes if n.kind != "File"]

    n_put = len(scoped_nodes)
    n_test = len(test_nodes)

    if n_put == 0 and n_test == 0:
        return TopologicalCoverageReport(
            topological_distance=0.0,
            topological_coverage_score=1.0,
            tested_functions=tuple(tested_funcs),
            untested_functions=tuple(untested_functions),
            put_node_count=len(put_cpg.nodes),
            test_node_count=len(test_cpg.nodes),
            scoped_node_count=0,
        )

    # Joint PCA needs at least 2 points across the union; for smaller cases
    # we fall back to a trivial distance based on size imbalance only.
    if n_put + n_test < 2:
        distance = float(abs(n_put - n_test))
        return TopologicalCoverageReport(
            topological_distance=distance,
            topological_coverage_score=float(np.exp(-distance)),
            tested_functions=tuple(tested_funcs),
            untested_functions=tuple(untested_functions),
            put_node_count=len(put_cpg.nodes),
            test_node_count=len(test_cpg.nodes),
            scoped_node_count=n_put,
        )

    model = get_embedding_model()
    put_texts = [
        put_cpg.node_text(n).strip() or f"{n.kind} {str(n.attributes)}"
        for n in scoped_nodes
    ]
    test_texts = [
        test_cpg.node_text(n).strip() or f"{n.kind} {str(n.attributes)}"
        for n in test_nodes
    ]
    unique_texts = list(set(put_texts + test_texts))
    raw_embeddings = list(model.embed(unique_texts))
    text_to_emb = dict(zip(unique_texts, raw_embeddings, strict=True))

    emb_put = _embed_nodes(put_cpg, scoped_nodes, text_to_emb)
    emb_test = _embed_nodes(test_cpg, test_nodes, text_to_emb)

    # Joint 2-D PCA so both ECTs live in the same frame.
    if n_put == 0:
        pts_test_only = (emb_test - emb_test.mean(axis=0, keepdims=True)).astype(
            np.float32
        )
        _, _, vt = np.linalg.svd(pts_test_only, full_matrices=False)
        pts_test = (pts_test_only @ vt[:2].T).astype(np.float32)
        pts_put = np.zeros((0, 2), dtype=np.float32)
    elif n_test == 0:
        pts_put_only = (emb_put - emb_put.mean(axis=0, keepdims=True)).astype(
            np.float32
        )
        _, _, vt = np.linalg.svd(pts_put_only, full_matrices=False)
        pts_put = (pts_put_only @ vt[:2].T).astype(np.float32)
        pts_test = np.zeros((0, 2), dtype=np.float32)
    else:
        pts_put, pts_test = _joint_pca_2d(emb_put, emb_test)

    # Stack into a single batched call so directions and ``lin`` are shared.
    points = np.vstack([pts_put, pts_test]).astype(np.float32)
    group_ids = np.concatenate(
        [np.zeros(n_put, dtype=np.int64), np.ones(n_test, dtype=np.int64)]
    )

    put_id_to_local = {node.id: i for i, node in enumerate(scoped_nodes)}
    test_id_to_local = {node.id: n_put + i for i, node in enumerate(test_nodes)}
    ei_put = _local_edge_index(put_cpg.edges, put_id_to_local)
    ei_test = _local_edge_index(test_cpg.edges, test_id_to_local)
    edge_index = np.concatenate([ei_put, ei_test], axis=1)
    if edge_index.size == 0:
        edge_index = np.zeros((2, 0), dtype=np.int64)

    # Joint filtration radius. ``compute_ect_from_numpy`` centers ``lin`` on 0
    # with width ``2 * radius``; pad the observed max projection slightly so
    # the curves don't clip on identical sources.
    max_abs = float(np.max(np.abs(points))) if points.size else 1.0
    radius = max(max_abs * 1.05, 1e-3)

    ect = compute_ect_from_numpy(
        points,
        group_ids=group_ids,
        edge_index=edge_index if edge_index.shape[1] > 0 else None,
        num_thetas=num_directions,
        resolution=num_steps,
        radius=radius,
    )
    # Shape: (2, resolution, num_thetas). Row 0 = PUT, row 1 = test.
    ect_put = ect[0]
    ect_test = ect[1]

    # Scale-invariant comparison: divide each curve by its respective node count.
    norm_put = max(n_put, 1)
    norm_test = max(n_test, 1)
    diff = ect_put / norm_put - ect_test / norm_test
    distance = float(np.sqrt(np.mean(diff**2)))
    # Batched ECT over identical graphs can still leave sub-nanometric native
    # float residue across platforms. Treat that as the mathematical zero.
    if np.isclose(distance, 0.0, rtol=0.0, atol=_ECT_ZERO_DISTANCE_TOLERANCE):
        distance = 0.0
    coverage_score = float(np.exp(-distance))

    return TopologicalCoverageReport(
        topological_distance=distance,
        topological_coverage_score=coverage_score,
        tested_functions=tuple(tested_funcs),
        untested_functions=tuple(untested_functions),
        put_node_count=len(put_cpg.nodes),
        test_node_count=len(test_cpg.nodes),
        scoped_node_count=n_put,
    )
