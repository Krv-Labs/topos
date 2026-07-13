"""Tests for the graphs package."""

from pathlib import Path

from topos.graphs.base import Representation
from topos.topos_functors import GraphNode, ModuleDependencyGraph

# ---------------------------------------------------------------------------
# Protocol conformance
# ---------------------------------------------------------------------------


def test_dependency_graph_conforms_to_protocol():
    graph = ModuleDependencyGraph.from_parts("foo.py", [], [])
    assert isinstance(graph, Representation)


# ---------------------------------------------------------------------------
# ASTRepresentation
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# ModuleDependencyGraph construction and lookups
# ---------------------------------------------------------------------------


def test_depgraph_name():
    g = ModuleDependencyGraph.from_parts("foo.py", [], [])
    assert g.name == "mdg"


def test_depgraph_metrics_no_file_found():
    g = ModuleDependencyGraph.from_parts("nonexistent.py", [], [])
    m = g.metrics()
    assert m["mdg.coupling"] == 0.0
    assert m["mdg.instability"] == 0.5


# ---------------------------------------------------------------------------
# ASTRepresentation — verdict dispatch via registry
# ---------------------------------------------------------------------------


def test_entrypoint_filename_hints_cover_supported_languages():
    from topos.evaluation.file_roles import _entrypoint_filename_hint

    assert _entrypoint_filename_hint(Path("__init__.py"), "python") is True
    assert _entrypoint_filename_hint(Path("mod.rs"), "rust") is True
    assert _entrypoint_filename_hint(Path("lib.rs"), "rust") is True
    assert _entrypoint_filename_hint(Path("index.ts"), "typescript") is True
    assert _entrypoint_filename_hint(Path("index.js"), "javascript") is True
    assert _entrypoint_filename_hint(Path("something.cpp"), "cpp") is False


def test_entrypoint_source_only_rejects_logic_for_entrypoint_files():
    from topos.evaluation.file_roles import _is_entrypoint_source_only

    assert _is_entrypoint_source_only("from .core import run\n", "python") is True
    assert (
        _is_entrypoint_source_only("from .core import run\nx = 1\n", "python") is False
    )
    assert (
        _is_entrypoint_source_only(
            "from . import (\n    assess,\n    evaluate,\n)\n", "python"
        )
        is True
    )
    assert (
        _is_entrypoint_source_only("from . import (\n    assess,\n)\nx = 1\n", "python")
        is False
    )
    assert _is_entrypoint_source_only("export * from './a'\n", "typescript") is True
    assert _is_entrypoint_source_only("export const x = 1\n", "typescript") is False
    assert (
        _is_entrypoint_source_only("// re-export\nexport * from './a'\n", "typescript")
        is True
    )
    assert (
        _is_entrypoint_source_only("#pragma once\n#include <vector>\n", "cpp") is True
    )
    assert _is_entrypoint_source_only("#include <vector>\nint x;\n", "cpp") is False
    assert _is_entrypoint_source_only("/// module\npub use crate::a;\n", "rust") is True


# ---------------------------------------------------------------------------
# Backward compatibility of metric imports
# ---------------------------------------------------------------------------


def test_backward_compat_top_level_imports():
    from topos import (
        ASTRepresentation,
        ModuleDependencyGraph,
        Representation,
    )

    assert ASTRepresentation is not None
    assert ModuleDependencyGraph is not None
    assert Representation is not None


# ---------------------------------------------------------------------------
# file_node_id — path-matching branch coverage
# ---------------------------------------------------------------------------


def _graph_with_file(
    target_file: str, file_path_property: str
) -> ModuleDependencyGraph:
    """Minimal graph with one File node for path-matching tests."""
    return ModuleDependencyGraph.from_parts(
        target_file,
        [
            GraphNode(
                id="File:node",
                label="File",
                properties={"filePath": file_path_property},
            )
        ],
        [],
    )


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
