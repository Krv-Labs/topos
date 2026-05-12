"""
Universal Abstract Syntax Tree (UAST)
-------------------------------------

This package implements the normalization layer that transforms language-specific
concrete syntax trees (CSTs) from Tree-sitter into a unified abstract
representation.

UAST follows a "Native-first, Normalized-second" philosophy:
1.  **Normalization**: Standardizes disparate language constructs (e.g., Python's
    'function_definition' vs JS's 'function_declaration') into unified kinds
    (e.g., 'FunctionDecl').
2.  **Fidelity**: Preserves exact source coordinates (SourceSpan) and native
    parser provenance (NativeRef).
3.  **Efficiency**: Leverages Tree-sitter for fast, incremental parsing while
    filtering out non-semantic noise like punctuation and whitespace.

The resulting trees are faithful to industry standards (Python ast, ESTree,
Rust syn, Clang) while providing a stable target for cross-language metrics
and agents.
"""

from topos.graphs.ast.uast.models import NativeRef, SourceSpan, UASTNode

__all__ = ["SourceSpan", "NativeRef", "UASTNode"]
