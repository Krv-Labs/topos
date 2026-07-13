//! Structural representations of a program — AST, UAST, CFG, CPG, MDG,
//! PDG, and process graphs — each implementing [`base::Representation`].
//!
//! Only [`base`] has landed so far. The concrete representations follow
//! in issues #142 (`uast`/`ast`) and #143 (`cfg`/`cpg`/`mdg`/`pdg`/`process`).

pub mod base;
