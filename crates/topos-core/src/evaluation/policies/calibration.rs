//! Policy calibration — central hub for evaluation gates and scoring
//! constants.
//!
//! Edit [`SIMPLE`], [`COMPOSABLE`], [`SECURE`] and [`score_floor`] when
//! updating from experimental data. All policy translators read from
//! this module; nothing else should define pass/fail or normalization
//! numbers.
//!
//! - **Raw-metric gates** drive `ScoredDecision.achieved` (AND
//!   semantics). Each `Φᵢ` compares probe values against these fields;
//!   they are the decisive pass/fail criteria for the three quality
//!   generators in `Ω`.
//! - **Normalization caps/scales** map raw metrics to `[0, 1]` quality
//!   scores for reporting and multi-file aggregation. They do **not**
//!   gate `achieved`.
//! - **Score floors** are the alternate path via
//!   `policies::base::meet_satisfied` and multi-file
//!   `CharacteristicMorphism` meets. Live `Φᵢ` translators don't use
//!   these for `achieved`.
//!
//! Calibration provenance: PyPI corpus ECDF calibration (June 2026). See
//! `topos-leaderboard/CALIBRATION_REPORT.md` and `calibration.json`.
//!
//! `CoveragePolicyThresholds`/`ClonePolicyThresholds` (auxiliary,
//! outside `Ω`) aren't ported yet — `policies::{clones,coverage}` are
//! remaining work within issue #144.

use crate::evaluation::preferences::Generator;

/// `Φ_SIMPLE` gates and normalization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimplePolicyThresholds {
    // Gates (achieved)
    pub max_cyclomatic: f64,
    pub max_function_complexity: f64,
    pub min_entropy: f64,
    pub max_entropy: f64,
    // Normalization (score only)
    pub max_cyclomatic_cap: f64,
    pub max_function_complexity_cap: f64,
    pub entropy_ideal: f64,
}

/// `Φ_COMPOSABLE` gates and normalization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposablePolicyThresholds {
    // Gates (achieved)
    pub instability_low: f64,
    pub instability_high: f64,
    pub max_fan_in: f64,
    pub max_fan_out: f64,
    /// Entrypoint carve-out: import/export-only entrypoint modules with
    /// zero fan-in may sit at or above this instability without
    /// failing the gate.
    pub entrypoint_instability_min: f64,
    // Normalization (score only)
    pub max_fan_in_cap: f64,
    pub max_fan_out_cap: f64,
}

/// `Φ_SECURE` gates and normalization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SecurePolicyThresholds {
    // Gates (achieved) — strict zero-tolerance security
    pub max_dangerous_calls: f64,
    pub max_taint_flows: f64,
    // Normalization (score only) — exponential decay scales
    pub danger_scale: f64,
    pub taint_scale: f64,
}

pub const SIMPLE: SimplePolicyThresholds = SimplePolicyThresholds {
    max_cyclomatic: 15.0,
    max_function_complexity: 10.0,
    min_entropy: 0.2,
    max_entropy: 0.8,
    max_cyclomatic_cap: 40.0,
    max_function_complexity_cap: 20.0,
    entropy_ideal: 0.5,
};

pub const COMPOSABLE: ComposablePolicyThresholds = ComposablePolicyThresholds {
    instability_low: 0.3,
    instability_high: 0.7,
    max_fan_in: 15.0,
    max_fan_out: 15.0,
    entrypoint_instability_min: 0.95,
    max_fan_in_cap: 40.0,
    max_fan_out_cap: 40.0,
};

pub const SECURE: SecurePolicyThresholds = SecurePolicyThresholds {
    max_dangerous_calls: 0.0,
    max_taint_flows: 0.0,
    danger_scale: 3.0,
    taint_scale: 3.0,
};

/// Score-floor alternate path (`meet_satisfied` + multi-file
/// `CharacteristicMorphism`).
pub fn score_floor(generator: Generator) -> f64 {
    match generator {
        Generator::Simple => 0.40,
        Generator::Composable => 0.80,
        Generator::Secure => 1.00,
    }
}
