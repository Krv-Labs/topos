//! Tool implementations, grouped as in the Python package: evaluate,
//! assess, compare, coverage, depgraph, docs, graphify, inspect,
//! preferences, refactor. Each module contributes a `#[tool_router]` block
//! on [`crate::server::ToposServer`]; `server.rs` sums them.

pub mod assess;
pub mod compare;
pub mod coverage;
pub mod depgraph;
pub mod docs;
pub mod evaluate;
pub mod graphify;
pub mod inspect;
pub mod preferences;
pub mod refactor;
