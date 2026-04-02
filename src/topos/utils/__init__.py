"""
Utils Module
------------
Infrastructure for AST parsing and normalization.

Provides a language-agnostic interface to tree-sitter, designed
for future extension to multiple programming languages.
"""

from topos.utils.tree_sitter import parse_python, PythonParser

__all__ = ["parse_python", "PythonParser"]
