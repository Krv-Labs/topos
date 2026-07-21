//! Structural representations of a program — AST, UAST, CFG, CPG, MDG,
//! PDG, and process graphs — each implementing [`base::Representation`].
//!
//! [`base`] and [`ast`]/[`uast`] have landed (issues #141, #142). The
//! remaining representations follow in issue #143
//! (`cfg`/`cpg`/`mdg`/`pdg`/`process`).
//!
//! [`graphify`] is not part of that list — it's an *advisory-only* graph
//! (issue #150), never a scored [`base::Representation`]; see its own doc
//! comment.

pub mod ast;
pub mod base;
pub mod cfg;
pub mod cpg;
pub mod graphify;
pub mod mdg;
pub mod pdg;
pub mod process;
pub mod uast;
