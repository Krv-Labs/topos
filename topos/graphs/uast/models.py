"""
UAST Models
-----------

Data structures for the Universal Abstract Syntax Tree. These models define the
"Normalized" layer of our "Native-first, Normalized-second" architecture.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class SourceSpan:
    file: str | None
    start_byte: int
    end_byte: int
    start_line: int
    start_column: int
    end_line: int
    end_column: int


@dataclass(frozen=True)
class NativeRef:
    parser: str
    parser_version: str
    node_kind: str


@dataclass
class UASTNode:
    """
    Language-normalized node carrying provenance and source spans.

    The UASTNode acts as a normalization layer over language-specific Concrete
    Syntax Trees (CSTs) from Tree-sitter. It maps disparate native nodes into
    unified `kind` values; representation limits and mapper boundaries are
    documented in openwiki/architecture/overview.md (UAST contract section).

    While normalized, each node strictly retains its `native` provenance and
    `span` data to ensure fidelity with compiler-native AST expectations
    (e.g., Python ast, ESTree, Rust syn, Clang).

    `id` is a deterministic 16-hex-char identifier required by the
    `UNodeBase` schema for referential integrity (diffs, refactor links,
    cross-tool references). It is a blake2b hash of
    `(lang, native.node_kind, span.start_byte, span.end_byte, parent_id)`;
    chaining the parent's id encodes the full path from the root, which
    disambiguates identical-span sibling nodes without needing an explicit
    sibling index. The mapper walker is responsible for populating it; if
    a node is constructed directly (e.g. in tests) and no id is supplied,
    it defaults to the empty string.
    """

    kind: str
    lang: str
    span: SourceSpan
    native: NativeRef
    attributes: dict[str, Any] = field(default_factory=dict)
    children: list[UASTNode] = field(default_factory=list)
    id: str = ""
