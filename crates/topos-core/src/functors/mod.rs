//! Functors and profunctors over program representations: probes
//! (metrics) and comparisons.
//!
//! [`probes`] holds the single-program probes `P : E → ℝ`; [`profunctors`]
//! holds the two-program comparisons `D : E × E^op → ℝ` (issue #145).

pub mod probes;
pub mod profunctors;
