//! Policy translators `Φᵢ : ℝ → Ω`, one per quality generator, plus the
//! shared types and calibration/gate machinery they're built on.
//!
//! `policies::{clones, coverage}` are auxiliary policies outside `Ω` —
//! pairwise clone detection and declaration-level test coverage,
//! neither of which feeds the SIMPLE / COMPOSABLE / SECURE lattice.

pub mod base;
pub mod calibration;
pub mod clones;
pub mod composable;
pub mod coverage;
pub mod gates;
pub mod secure;
pub mod simple;
