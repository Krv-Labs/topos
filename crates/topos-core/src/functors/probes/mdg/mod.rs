//! Module Dependency Graph (MDG) probes — `P : E → ℝ` restricted to the
//! inter-module dependency-graph functor's image.
//!
//! Probes consumed by `Φ_COMPOSABLE` to score the COMPOSABLE generator:
//! - Coupling (afferent/efferent) and Martin instability
//! - Fan-in / fan-out
//! - Dependency depth
//!
//! Plus one advisory probe (never folded into COMPOSABLE):
//! - [`curvature`] — balanced Forman curvature for `topos refactor dependencies`.

pub mod coupling;
pub mod curvature;
pub mod fan;
