//! AST representation — adapts [`crate::core::object::ProgramObject`] to
//! the [`crate::graphs::base::Representation`] trait.
//!
//! This does *not* replace `ProgramObject`; it wraps it so the (pending,
//! issue #144) characteristic morphism can treat it uniformly alongside
//! other representations (dependency graph, CFG, etc.).
//!
//! The metrics this representation produces:
//! - `ast.entropy` — Kolmogorov proxy via compression ratio
//! - `ast.max_function_complexity` — cyclomatic complexity of the most
//!   complex function
//!
//! Both come from `functors::probes::ast::{entropy, complexity}`, which
//! land in issue #145 — [`AstRepresentation::metrics`] is a documented
//! stub until then.

use std::collections::HashMap;

use crate::core::object::ProgramObject;
use crate::graphs::base::Representation;

/// Wraps a [`ProgramObject`] and its source text, exposing complexity
/// and entropy as representation-level metrics.
pub struct AstRepresentation<'a> {
    pub program_object: &'a ProgramObject,
}

impl<'a> AstRepresentation<'a> {
    pub fn new(program_object: &'a ProgramObject) -> Self {
        AstRepresentation { program_object }
    }
}

impl Representation for AstRepresentation<'_> {
    fn name(&self) -> &str {
        "ast"
    }

    /// Feeds the SIMPLE generator via `ast.entropy`. Cyclomatic
    /// complexity is produced by the CFG representation (issue #143).
    fn dimension(&self) -> &str {
        "simple"
    }

    fn metrics(&self) -> HashMap<String, f64> {
        unimplemented!(
            "AstRepresentation::metrics depends on functors::probes::ast::{{entropy, complexity}} (issue #145)"
        )
    }
}
