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

#[cfg(test)]
mod tests {
    use super::*;

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
