//! UAST probes — single-program structural signatures over UAST kinds.
//!
//! Pairwise comparison and structural test-coverage (two-program
//! operations) live in [`crate::functors::profunctors::uast`]; this
//! module holds only the per-program probes `P : E → ℝ` that those
//! profunctors compose.

pub mod signature;
