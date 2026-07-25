//! `Representation` — the protocol every structural view of a program
//! must satisfy.
//!
//! In the topos of programs, a program can be viewed through multiple
//! lenses: its AST structure, its dependency graph, its control-flow
//! graph, etc. Each lens is a *representation* — a distinct categorical
//! object that captures different structural invariants of the same
//! morphism.
//!
//! Every representation can produce a map of metric values. These
//! metrics are routed through representation-specific evaluation
//! sections and ultimately aggregated by the lattice into a single
//! verdict.

use std::collections::HashMap;

/// Trait every program representation must implement.
///
/// A representation is a structural view of a program (AST, dependency
/// graph, CFG, ...) that can be measured along its own metric axes. The
/// Python original is a `@runtime_checkable Protocol`; Rust has no
/// structural-typing equivalent, so this is an explicit trait that each
/// representation (`graphs::cfg::ControlFlowGraph`, etc. — issue #143)
/// implements and stores behind `Box<dyn Representation>` wherever the
/// Python side held a duck-typed `Representation`.
pub trait Representation {
    /// A unique identifier for this representation type (e.g. `"ast"`,
    /// `"mdg"`).
    fn name(&self) -> &str;

    /// The quality axis this representation measures.
    ///
    /// Representations with the same dimension are aggregated together
    /// within a single dimension verdict via lattice meet. Representations
    /// with different dimensions are reported separately and never
    /// collapsed into each other.
    ///
    /// Standard dimension names:
    /// - `"structural"` — internal code structure (AST-based)
    /// - `"coupling"` — architectural positioning (dependency-graph)
    fn dimension(&self) -> &str;

    /// Compute all metric values for this representation.
    ///
    /// Keys should be namespaced by representation (e.g.
    /// `"ast.complexity"`, `"mdg.coupling"`).
    fn metrics(&self) -> HashMap<String, f64>;
}
