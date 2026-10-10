//! Gate-anchored scoring primitives shared by every `Φᵢ`.
//!
//! A pillar's continuous score is derived from its gates so it can never
//! contradict the verdict (see `docs/decisions/gate-anchored-scoring.md`):
//!
//! ```text
//! d_i  = desirability of gated metric i, anchored so d_i = TAU at its gate
//! G    = min_i d_i                       (Gödel t-norm; pass ⇔ G ≥ TAU)
//! A    ∈ (0, 1]                          (codebase-relative advisory score)
//! S    = TAU + (G − TAU)·A^W_A   if G ≥ TAU
//!      = G·A^W_A                 otherwise
//! ```
//!
//! Advisories move `S` within the half the verdict chose, never across `TAU`.
//! With no advisories (`A = 1`) the score is exactly `G`.

/// Desirability at a gate threshold: `d ≥ TAU` ⇔ the gate passes.
pub const TAU: f64 = 0.5;

/// Exponent on the advisory score: how far advisories may move a pillar
/// score inside its band. The one judgment-call constant of the scheme.
pub const W_A: f64 = 1.0 / 3.0;

/// Combine a pillar's gate score `g` with its advisory score `a`.
///
/// `g` and `a` are clamped to `[0, 1]`; `a` is floored at a small epsilon so
/// a single extreme advisory cannot zero a passing pillar's margin silently.
pub fn band_score(g: f64, a: f64) -> f64 {
    let g = g.clamp(0.0, 1.0);
    let discount = a.clamp(1e-3, 1.0).powf(W_A);
    if g >= TAU {
        TAU + (g - TAU) * discount
    } else {
        g * discount
    }
}

/// Lower-is-better desirability with gate `g`: `1` at `0`, `TAU` at `g`,
/// `0` at `2g` and beyond. Piecewise linear; `NaN` scores `0` (the gate
/// fails closed on `NaN` too).
pub fn d_lower_is_better(v: f64, g: f64) -> f64 {
    if v.is_nan() {
        return 0.0;
    }
    let v = v.max(0.0);
    if v <= g {
        1.0 - TAU * v / g
    } else {
        (TAU * (2.0 * g - v) / g).max(0.0)
    }
}

/// Band desirability over the unit interval: `1` at `ideal`, `TAU` at
/// each band edge (`low`, `high`), `0` at `0.0` and at `1.0`. Piecewise
/// linear; `NaN` scores `0`.
pub fn d_band(v: f64, low: f64, ideal: f64, high: f64) -> f64 {
    if v.is_nan() {
        return 0.0;
    }
    let d = if v < low {
        TAU * v / low
    } else if v <= ideal {
        TAU + TAU * (v - low) / (ideal - low)
    } else if v <= high {
        TAU + TAU * (high - v) / (high - ideal)
    } else {
        TAU * (1.0 - v) / (1.0 - high)
    };
    d.clamp(0.0, 1.0)
}

/// Zero-tolerance desirability: `1` at `0`, `TAU·exp(−v/scale)` above it,
/// so any positive count lands strictly below `TAU`. `NaN` scores `0`.
pub fn d_zero_tolerance(v: f64, scale: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else if v <= 0.0 {
        1.0
    } else {
        TAU * (-v / scale).exp()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lower_is_better_anchors() {
        assert_eq!(d_lower_is_better(0.0, 10.0), 1.0);
        assert_eq!(d_lower_is_better(10.0, 10.0), TAU);
        assert_eq!(d_lower_is_better(20.0, 10.0), 0.0);
        assert_eq!(d_lower_is_better(50.0, 10.0), 0.0);
        assert!((d_lower_is_better(8.0, 10.0) - 0.6).abs() < 1e-12);
        assert!(d_lower_is_better(10.0001, 10.0) < TAU);
        assert_eq!(d_lower_is_better(f64::NAN, 10.0), 0.0);
    }

    #[test]
    fn band_anchors() {
        assert_eq!(d_band(0.5, 0.2, 0.5, 0.8), 1.0);
        assert!((d_band(0.2, 0.2, 0.5, 0.8) - TAU).abs() < 1e-12);
        assert!((d_band(0.8, 0.2, 0.5, 0.8) - TAU).abs() < 1e-12);
        assert_eq!(d_band(0.0, 0.2, 0.5, 0.8), 0.0);
        assert!(d_band(1.0, 0.2, 0.5, 0.8).abs() < 1e-12);
        assert!(d_band(0.1999, 0.2, 0.5, 0.8) < TAU);
        assert!(d_band(0.8001, 0.2, 0.5, 0.8) < TAU);
        assert_eq!(d_band(f64::NAN, 0.2, 0.5, 0.8), 0.0);
    }

    #[test]
    fn zero_tolerance_anchors() {
        assert_eq!(d_zero_tolerance(0.0, 3.0), 1.0);
        assert!(d_zero_tolerance(1.0, 3.0) < TAU);
        assert!(d_zero_tolerance(1.0, 3.0) > d_zero_tolerance(2.0, 3.0));
        assert_eq!(d_zero_tolerance(f64::NAN, 3.0), 0.0);
    }

    #[test]
    fn no_advisories_is_identity() {
        for g in [0.0, 0.2, 0.5, 0.73, 1.0] {
            assert!((band_score(g, 1.0) - g).abs() < 1e-12);
        }
    }

    #[test]
    fn advisories_never_cross_tau() {
        for g in [0.5, 0.6, 1.0] {
            assert!(band_score(g, 1e-9) >= TAU);
        }
        for g in [0.0, 0.3, 0.4999] {
            assert!(band_score(g, 1.0) < TAU);
        }
    }

    #[test]
    fn worse_advisories_lower_the_score() {
        assert!(band_score(0.9, 0.2) < band_score(0.9, 0.8));
    }
}
