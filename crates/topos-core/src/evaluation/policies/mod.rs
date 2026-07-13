//! Policy translators `Φᵢ : ℝ → Ω`, one per quality generator, plus the
//! shared types and calibration/gate machinery they're built on.
//!
//! `policies::{clones, coverage}` (auxiliary, outside `Ω`) aren't ported
//! yet — remaining work within issue #144.

pub mod base;
pub mod calibration;
pub mod composable;
pub mod gates;
pub mod secure;
pub mod simple;
