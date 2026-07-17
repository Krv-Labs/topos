"""Guard tests for lazy CLI command registration."""

from __future__ import annotations

import subprocess
import sys


def test_register_commands_does_not_import_tree_sitter() -> None:
    """Importing language constants for Click options must not load tree-sitter."""
    script = """
import sys

from topos.cli.main import _register_commands

_register_commands()

tree_sitter = [
    name for name in sys.modules
    if name == "tree_sitter" or name.startswith("tree_sitter_")
]
assert not tree_sitter, f"unexpected tree_sitter imports: {tree_sitter}"
"""
    completed = subprocess.run(
        [sys.executable, "-c", script],
        capture_output=True,
        text=True,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr


def test_register_commands_does_not_import_numpy_or_evaluation() -> None:
    """Registering commands (incl. quality.py's ``--priority`` option) must
    not eagerly pull in numpy or the evaluation stack. Those are heavy and
    only needed once a command actually evaluates code, inside lazy-imported
    callbacks (see issue #119)."""
    script = """
import sys

from topos.cli.main import _register_commands

_register_commands()

heavy = [
    name for name in sys.modules
    if name == "numpy" or name.startswith("numpy.")
    or name == "topos.evaluation" or name.startswith("topos.evaluation.")
]
assert not heavy, f"unexpected eager imports: {heavy}"
"""
    completed = subprocess.run(
        [sys.executable, "-c", script],
        capture_output=True,
        text=True,
    )
    assert completed.returncode == 0, completed.stdout + completed.stderr
