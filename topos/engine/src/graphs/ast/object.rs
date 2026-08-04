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
//! Both come from `functors::probes::ast::{entropy, complexity}` — pulled
//! forward from issue #145 as a dependency of #144's
//! `CharacteristicMorphism`, which always builds an `AstRepresentation`.

use std::collections::HashMap;

use crate::core::object::ProgramObject;
use crate::functors::probes::ast::complexity::calculate_max_function_complexity;
use crate::functors::probes::ast::divergence::calculate_max_function_divergence;
use crate::functors::probes::ast::entropy::calculate_kolmogorov_proxy;
use crate::graphs::base::Representation;
use crate::graphs::uast::models::UASTNode;

/// Wraps a [`ProgramObject`] and its source text, exposing complexity
/// and entropy as representation-level metrics.
pub struct AstRepresentation<'a> {
    pub program_object: &'a ProgramObject,
    pub source: &'a str,
    pub uast_root: &'a UASTNode,
}

impl<'a> AstRepresentation<'a> {
    pub fn new(
        program_object: &'a ProgramObject,
        source: &'a str,
        uast_root: &'a UASTNode,
    ) -> Self {
        AstRepresentation {
            program_object,
            source,
            uast_root,
        }
    }
}

/// Wraps the same UAST as [`AstRepresentation`], but exposes the
/// NAVIGABLE dimension.
///
/// A [`Representation`] declares exactly one `dimension()`, so the
/// divergence metric cannot ride along on `AstRepresentation` even though
/// it comes off the same parse. It lives here rather than in a `graphs/nav/`
/// module of its own because there is no new *graph* — just a second
/// reading of the AST.
pub struct NavigableRepresentation<'a> {
    pub uast_root: &'a UASTNode,
}

impl<'a> NavigableRepresentation<'a> {
    pub fn new(uast_root: &'a UASTNode) -> Self {
        NavigableRepresentation { uast_root }
    }
}

impl Representation for NavigableRepresentation<'_> {
    fn name(&self) -> &str {
        "nav"
    }

    /// Feeds the NAVIGABLE generator via `nav.max_function_divergence`.
    fn dimension(&self) -> &str {
        "navigable"
    }

    fn metrics(&self) -> HashMap<String, f64> {
        HashMap::from([(
            "nav.max_function_divergence".to_string(),
            calculate_max_function_divergence(self.uast_root),
        )])
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
        let _ = self.program_object; // reserved: node_count/depth-based metrics, if any land later
        HashMap::from([
            (
                "ast.entropy".to_string(),
                calculate_kolmogorov_proxy(self.source),
            ),
            (
                "ast.max_function_complexity".to_string(),
                calculate_max_function_complexity(self.uast_root) as f64,
            ),
        ])
    }
}
