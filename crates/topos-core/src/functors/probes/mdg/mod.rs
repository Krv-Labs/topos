//! Module Dependency Graph (MDG) probes — `P : E → ℝ` restricted to the
//! inter-module dependency-graph functor's image.
//!
//! Probes consumed by `Φ_COMPOSABLE` to score the COMPOSABLE generator:
//! - Coupling (afferent/efferent) and Martin instability
//! - Fan-in / fan-out
//! - Dependency depth

pub mod coupling;
pub mod fan;
