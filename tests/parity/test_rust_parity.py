"""
Parity tests comparing current Rust implementations against v1.0.0 Python baseline.
"""

import pytest
from topos.functors.probes.ast.entropy import calculate_kolmogorov_proxy
from topos.functors.probes.cfg.complexity import (
    cyclomatic_complexity,
    essential_complexity,
    max_nesting_depth,
)
from topos.functors.probes.cfg.paths import longest_acyclic_path
from topos.functors.profunctors.ast.compare import _compute_sequence_distance
from topos.graphs.cfg.builder import build_cfg_from_uast
from topos.graphs.cfg.object import ControlFlowGraph
from topos.graphs.uast.mapper_python import map_python_tree_to_uast
from topos.utils.tree_sitter import parse_python

from tests.parity.baseline_v1 import (
    calculate_kolmogorov_proxy_v1,
    compute_sequence_distance_v1,
    cyclomatic_complexity_v1,
    essential_complexity_v1,
    longest_acyclic_path_v1,
    max_nesting_depth_v1,
)

# --- Test Data ---

SIMPLE_PYTHON = "def f(x): return x + 1"
BRANCHY_PYTHON = """
def complex_func(x):
    if x > 0:
        for i in range(x):
            if i % 2 == 0:
                print(i)
            else:
                continue
    else:
        try:
            raise ValueError("bad")
        except:
            return -1
    return 0
"""


@pytest.fixture
def branchy_cfg():
    root = parse_python(BRANCHY_PYTHON)
    uast = map_python_tree_to_uast(root)
    blocks, edges, entry_id, exit_id = build_cfg_from_uast(uast)
    return ControlFlowGraph(
        blocks=blocks, edges=edges, entry_id=entry_id, exit_id=exit_id
    )


# --- CFG Parity ---


def test_cyclomatic_complexity_parity(branchy_cfg):
    rust_val = cyclomatic_complexity(branchy_cfg)
    py_val = cyclomatic_complexity_v1(branchy_cfg)
    assert rust_val == py_val


def test_essential_complexity_parity(branchy_cfg):
    rust_val = essential_complexity(branchy_cfg)
    py_val = essential_complexity_v1(branchy_cfg)
    assert rust_val == py_val


def test_max_nesting_depth_parity(branchy_cfg):
    rust_val = max_nesting_depth(branchy_cfg)
    py_val = max_nesting_depth_v1(branchy_cfg)
    assert rust_val == py_val


def test_longest_acyclic_path_parity(branchy_cfg):
    rust_val = longest_acyclic_path(branchy_cfg)
    py_val = longest_acyclic_path_v1(branchy_cfg)
    assert rust_val == py_val


# --- AST Parity ---


def test_entropy_parity():
    source = BRANCHY_PYTHON
    rust_val = calculate_kolmogorov_proxy(source)
    py_val = calculate_kolmogorov_proxy_v1(source)
    # We allow a small tolerance due to library heuristics (zlib vs flate2)
    assert rust_val == pytest.approx(py_val, abs=1e-3)


# --- Profunctor Parity ---


def test_edit_distance_parity():
    s1 = ["FunctionDecl", "IfStmt", "ReturnStmt", "ExprStmt"]
    s2 = ["FunctionDecl", "IfStmt", "ExprStmt", "ReturnStmt", "ExprStmt"]

    rust_dist, rust_ops = _compute_sequence_distance(s1, s2)
    py_dist, py_ops = compute_sequence_distance_v1(s1, s2)

    assert rust_dist == py_dist
    assert rust_ops == py_ops
