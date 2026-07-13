//! UAST models — the "Normalized" layer of Topos's "native-first,
//! normalized-second" architecture.
//!
//! Data structures for the Universal Abstract Syntax Tree: a
//! language-neutral tree that every `graphs::uast::mapper_*` module
//! produces from a language-specific tree-sitter CST, and that every
//! downstream structural probe (CFG/CPG/PDG builders, issue #143)
//! consumes uniformly regardless of source language.

use std::collections::HashMap;

/// A byte/line/column range in a source file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub file: Option<String>,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// Provenance of the parser that produced a native node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NativeRef {
    pub parser: String,
    pub parser_version: String,
    pub node_kind: String,
}

/// A UAST node attribute value.
///
/// Narrows Python's `dict[str, Any]` — the two concrete uses seen so far
/// are `mapper_common`'s `"named": bool` and `graphs::cfg::builder`'s
/// synthetic module-callable node (`"synthetic": bool`, `"scope": str`).
/// Widen with another variant if a future attribute needs a richer value.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    Bool(bool),
    Str(String),
}

/// Language-normalized node carrying provenance and source spans.
///
/// `UASTNode` acts as a normalization layer over language-specific
/// concrete syntax trees (CSTs) from tree-sitter. It maps disparate
/// native nodes into unified `kind` values that follow the
/// industry-standard reference in `docs/uast-industry-standards.md`.
///
/// While normalized, each node strictly retains its `native` provenance
/// and `span` data to ensure fidelity with compiler-native AST
/// expectations (e.g. Python `ast`, ESTree, Rust `syn`, Clang).
///
/// `id` is a deterministic 16-hex-char identifier: a BLAKE2b-8-byte hash
/// of `(lang, native.node_kind, span.start_byte, span.end_byte,
/// parent_id)` (see `mapper_common::compute_node_id`). Chaining the
/// parent's id encodes the full path from the root, which disambiguates
/// identical-span sibling nodes without needing an explicit sibling
/// index. The mapper walker populates it; a node built directly (e.g. in
/// tests) with no id supplied defaults to the empty string.
#[derive(Debug, Clone, PartialEq)]
pub struct UASTNode {
    pub kind: String,
    pub lang: String,
    pub span: SourceSpan,
    pub native: NativeRef,
    pub attributes: HashMap<String, AttributeValue>,
    pub children: Vec<UASTNode>,
    pub id: String,
}
