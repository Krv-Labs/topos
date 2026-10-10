//! Policy calibration — central hub for evaluation gates and scoring
//! constants.
//!
//! Edit [`SIMPLE`], [`COMPOSABLE`], [`SECURE`] and [`NAVIGABLE`] when
//! updating from experimental data. All policy translators read from
//! this module; nothing else should define pass/fail or desirability
//! numbers.
//!
//! - **Raw-metric gates** drive `ScoredDecision.achieved` (AND
//!   semantics). Each `Φᵢ` compares probe values against these fields;
//!   they are the decisive pass/fail criteria for the four quality
//!   generators in `Ω`.
//! - **Desirability anchors** — the gate thresholds themselves, plus
//!   `entropy_ideal` and the SECURE decay scales — shape each gated
//!   metric's desirability so it equals `TAU` exactly at the gate (see
//!   [`crate::evaluation::policies::desirability`] and
//!   `docs/decisions/gate-anchored-scoring.md`).
//! - **Score floors** are the alternate path via
//!   `policies::base::meet_satisfied`. Because scores are gate-anchored,
//!   every floor is `TAU`.
//!
//! Advisory metrics (`cfg.cyclomatic`, `mdg.instability`, `mdg.fan_in`,
//! …) have no fixed bands here; they are read relative to their codebase
//! by [`crate::evaluation::advisory`].
//!
//! Calibration provenance: PyPI corpus ECDF calibration (June 2026). See
//! `topos-leaderboard/CALIBRATION_REPORT.md` and `calibration.json`.
//!
//! `CoveragePolicyThresholds`/`ClonePolicyThresholds` — auxiliary,
//! outside `Ω` — back `policies::{clones,coverage}` (issue #145).

use crate::evaluation::policies::desirability::TAU;
use crate::evaluation::preferences::Generator;

/// `Φ_SIMPLE` gates and desirability anchors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimplePolicyThresholds {
    /// Not a gate: `cfg.cyclomatic` is advisory (issue #193). MCP still uses
    /// it as the cutoff for ranking "maintainability giants" and for the
    /// whole-file cyclomatic refactor location.
    pub max_cyclomatic: f64,
    // Gates (achieved)
    pub max_function_complexity: f64,
    pub min_entropy: f64,
    pub max_entropy: f64,
    /// Band-desirability peak for `ast.entropy`.
    pub entropy_ideal: f64,
    /// Below this many source bytes, an `ast.entropy` reading *above*
    /// `entropy_ideal` is unreliable — zlib's fixed per-stream overhead
    /// dominates the ratio (issue #152), so a tiny branch-free function can
    /// read as "denser" than a larger, genuinely branchy one. Mirrors
    /// `ENTROPY_SIZE_FLOOR_BYTES` in `functors::probes::ast::entropy`; see
    /// `evaluation::policies::simple::desirability`.
    pub entropy_size_floor_bytes: f64,
}

/// `Φ_COMPOSABLE` file-level gate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComposablePolicyThresholds {
    pub max_fan_out: f64,
}

/// `Φ_SECURE` gates and desirability decay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SecurePolicyThresholds {
    // Gates (achieved) — strict zero-tolerance security
    pub max_dangerous_calls: f64,
    pub max_taint_flows: f64,
    /// Desirability decay scales: `d = TAU·exp(−v/scale)` for `v > 0`.
    pub danger_scale: f64,
    pub taint_scale: f64,
}

pub const SIMPLE: SimplePolicyThresholds = SimplePolicyThresholds {
    max_cyclomatic: 15.0,
    max_function_complexity: 10.0,
    min_entropy: 0.2,
    max_entropy: 0.8,
    entropy_ideal: 0.5,
    entropy_size_floor_bytes: 200.0,
};

pub const COMPOSABLE: ComposablePolicyThresholds = ComposablePolicyThresholds {
    // Fresh v0.5 file-level calibration (2026-08-07): 2,979 production files
    // from Python, Rust, TypeScript, and the polyglot MCP cohort, after
    // excluding test/example paths. A cap of 10 failed 1.2% / 3.0% / 6.3% /
    // 6.8% respectively, or 4.3% with equal ecosystem weight. This is an
    // empirical Topos policy, not a universal constant from the literature.
    max_fan_out: 10.0,
};

pub const SECURE: SecurePolicyThresholds = SecurePolicyThresholds {
    max_dangerous_calls: 0.0,
    max_taint_flows: 0.0,
    danger_scale: 3.0,
    taint_scale: 3.0,
};

/// `Φ_NAVIGABLE` gate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NavigablePolicyThresholds {
    /// Worst-function Semantic Compositional Divergence a file may carry.
    ///
    /// Calibrated 2026-08-07 on a balanced 6,390-file leaderboard corpus
    /// (equal strata per ecosystem: PyPI, Cargo, npm, C++, Go, MCP): p50 `0.0`,
    /// p95 `10.37`. Gate `10.0` yields ~5.2% failure (MCP ~5.8%, Python ~6.0%),
    /// matching the ~5.5% calibration target used for SIMPLE and SECURE.
    pub max_function_divergence: f64,
}

pub const NAVIGABLE: NavigablePolicyThresholds = NavigablePolicyThresholds {
    max_function_divergence: 10.0,
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

/// Score-floor alternate path (`meet_satisfied`).
///
/// Scores are gate-anchored (`score ≥ TAU` ⇔ the pillar passed), so the
/// floor is `TAU` for every generator.
pub fn score_floor(_generator: Generator) -> f64 {
    TAU
}
