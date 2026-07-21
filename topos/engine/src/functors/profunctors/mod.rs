//! Profunctors `D : E × E^op → ℝ`.
//!
//! A *profunctor* in the program topos takes two programs (one
//! covariant, one contravariant) and returns a real-valued distance /
//! divergence. Where probes (in [`crate::functors::probes`]) measure a
//! *single* program against a fixed scale, profunctors measure the
//! *gap* between two programs viewed through a chosen representation.
//!
//! One submodule per representation:
//!
//! - [`ast`] — tree edit distance + Gromov-Wasserstein over the raw AST
//! - [`uast`] — kind-histogram + structural-summary deltas (cross-language)
//! - [`cfg`] — cyclomatic / edge-kind / longest-path deltas
//! - [`pdg`] — DDG and CDG edge-set Jaccards
//! - [`mdg`] — coupling, instability, fan-in/out, dep-depth deltas
//! - [`cpg`] — per-family edge Jaccards + danger / taint deltas

pub mod ast;
pub mod cfg;
pub mod cpg;
pub mod mdg;
pub mod pdg;
pub mod uast;
