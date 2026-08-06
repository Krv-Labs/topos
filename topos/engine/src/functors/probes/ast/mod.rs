//! AST-level probes. Entropy and per-function complexity feed the SIMPLE
//! generator; per-function compositional divergence feeds NAVIGABLE. All
//! three enumerate callables through the shared [`scopes`] walk.

pub mod complexity;
pub mod divergence;
pub mod entropy;
pub(crate) mod scopes;
