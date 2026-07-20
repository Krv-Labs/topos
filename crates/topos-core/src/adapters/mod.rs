//! Adapters — the edges of the categorical model that touch the outside
//! world: the filesystem (source-file discovery) and an external process
//! (the GitNexus dependency-graph CLI). Everything category-theoretic
//! lives in [`crate::core`]/[`crate::graphs`]/[`crate::evaluation`]/
//! [`crate::functors`]; this module is where that model meets `std::fs`
//! and `std::process`.
//!
//! - [`discovery`] — walk a directory tree for source files by language,
//!   pruning venvs, VCS/build noise, and `.toposignore`/`git`-ignored
//!   paths. Ported from `topos/utils/discovery.py`.
//! - [`gitnexus`] — shell out to `gitnexus analyze` to (re)generate a
//!   `.gitnexus/` dependency-graph store, with source fingerprinting to
//!   detect staleness. Ported from `topos/utils/gitnexus.py`.
//! - [`graphify`] — shell out to `graphify update`/`extract` to (re)generate
//!   a `graphify-out/graph.json` knowledge-graph store, a second external
//!   structural-analysis tool alongside GitNexus (issue #150). Advisory
//!   only — feeds `topos refactor graphify`, never the SIMPLE/COMPOSABLE/
//!   SECURE lattice; see [`crate::graphs::graphify`].
//!
//! `topos/utils/tree_sitter.py` is deliberately *not* ported here: every
//! per-language parser it sets up, and every helper it exposes
//! (`node_text`, `node_to_sexp`, `find_errors`'s underlying primitives),
//! is already covered by [`crate::graphs::ast::dispatch`] and by
//! `tree_sitter::Node`'s own `utf8_text`/`to_sexp`/`has_error`/`is_missing`
//! methods — see `dispatch`'s module doc for why there's exactly one
//! parsing path in this crate. See this issue's report for the full
//! redundancy analysis.

mod process;

pub mod discovery;
pub mod gitnexus;
pub mod graphify;
