//! Evaluation: the policy translators `Φᵢ` that feed the characteristic
//! morphism `χ_S : P → Ω` (which lives with the rest of the categorical
//! core in [`crate::core::characteristic_morphism`]).
//!
//! - [`policies`] — the `Φᵢ` translators and their calibration/gates.
//! - [`file_roles`] — structural exemptions (entrypoint modules, etc.).
//! - [`preferences`] — [`preferences::UserPreferences`], the strict total
//!   order over the three generators and its `Ω` relaxation walk.
//! - [`security_guidance`] — remediation prose/operations for dangerous
//!   APIs and taint flows.
//! - [`suggestions`] — turns a
//!   [`crate::core::characteristic_morphism::ClassificationResult`] into
//!   actionable refactor suggestions.
//! - [`suppression`] — the allowlist overlay that computes an *adjusted*
//!   SECURE verdict on top of the canonical one (anti-gaming design).
//!
//! Issue #144 (`topos/evaluation`) is now fully landed.

use std::collections::HashMap;

pub mod file_roles;
pub mod policies;
pub mod preferences;
pub mod security_guidance;
pub mod suggestions;
pub mod suppression;

/// Rank a file by its weakest measured pillar. Missing measurements sort first.
pub fn weakest_score(scores: &HashMap<String, f64>) -> f64 {
    scores.values().copied().reduce(f64::min).unwrap_or(0.0)
}

#[cfg(test)]
mod ranking_tests {
    use super::*;

    #[test]
    fn weakest_dimension_controls_rank() {
        let scores = HashMap::from([
            ("simple".to_string(), 0.8),
            ("composable".to_string(), 0.2),
            ("secure".to_string(), 1.0),
        ]);
        assert_eq!(weakest_score(&scores), 0.2);
        assert_eq!(weakest_score(&HashMap::new()), 0.0);
    }
}
