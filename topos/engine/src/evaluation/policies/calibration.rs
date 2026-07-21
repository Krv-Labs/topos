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
//! `CoveragePolicyThresholds`/`ClonePolicyThresholds` — auxiliary,
//! outside `Ω` — back `policies::{clones,coverage}` (issue #145).

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
    /// Below this many source bytes, an `ast.entropy` reading *above*
    /// `entropy_ideal` is unreliable — zlib's fixed per-stream overhead
    /// dominates the ratio (issue #152), so a tiny branch-free function can
    /// read as "denser" than a larger, genuinely branchy one. Mirrors
    /// `ENTROPY_SIZE_FLOOR_BYTES` in `functors::probes::ast::entropy`; see
    /// `evaluation::policies::simple::quality`.
    pub entropy_size_floor_bytes: f64,
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
    /// Distance from Martin's Main Sequence (D = |A + I - 1|), gated in
    /// place of raw instability whenever Abstractness (`mdg.abstractness`)
    /// is available — see `evaluation::policies::composable::score_coupling`
    /// and issue #124. PROVISIONAL: a first-pass estimate (roughly Martin's
    /// commonly-cited "principal zone" radius), not yet run through the
    /// PyPI corpus ECDF calibration the other constants in this struct
    /// received.
    pub main_sequence_distance_max: f64,
    /// Zone-of-Pain carve-out: a declarations-only, no-branching "stable
    /// leaf" module (constants, error types — see
    /// `evaluation::file_roles::is_stable_leaf_module`) may sit at or below
    /// this instability without failing the gate, mirroring
    /// `entrypoint_instability_min` for the low-instability extreme. Also
    /// PROVISIONAL.
    pub stable_leaf_instability_max: f64,
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
    entropy_size_floor_bytes: 200.0,
};

pub const COMPOSABLE: ComposablePolicyThresholds = ComposablePolicyThresholds {
    instability_low: 0.3,
    instability_high: 0.7,
    max_fan_in: 15.0,
    max_fan_out: 15.0,
    entrypoint_instability_min: 0.95,
    main_sequence_distance_max: 0.5,
    stable_leaf_instability_max: 0.05,
    max_fan_in_cap: 40.0,
    max_fan_out_cap: 40.0,
};

pub const SECURE: SecurePolicyThresholds = SecurePolicyThresholds {
    max_dangerous_calls: 0.0,
    max_taint_flows: 0.0,
    danger_scale: 3.0,
    taint_scale: 3.0,
};

/// Structural test-coverage policy (outside `Ω`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoveragePolicyThresholds {
    pub declaration_recall: f64,
    /// "strong" band above gate.
    pub strong_offset: f64,
    /// "partial" band = gate × this.
    pub partial_factor: f64,
}

/// Pairwise clone detection (outside `Ω`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClonePolicyThresholds {
    pub max_normalized_distance: f64,
}

pub const COVERAGE: CoveragePolicyThresholds = CoveragePolicyThresholds {
    declaration_recall: 0.5,
    strong_offset: 0.25,
    partial_factor: 0.5,
};

pub const CLONE: ClonePolicyThresholds = ClonePolicyThresholds {
    max_normalized_distance: 0.1,
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
