//! The Universal AST — the "Normalized" layer over language-specific
//! tree-sitter CSTs.
//!
//! [`models`] defines the tree ([`models::UASTNode`]); [`mapper_common`]
//! is the shared transformation engine (traversal, id hashing, the
//! per-language [`mapper_common::TestNodeFilter`] hook); one
//! `mapper_*` module per supported language builds on it.

pub mod mapper_common;
pub mod mapper_cpp;
pub mod mapper_go;
pub mod mapper_javascript;
pub mod mapper_python;
pub mod mapper_rust;
pub mod mapper_typescript;
pub mod models;
