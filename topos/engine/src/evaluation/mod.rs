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

pub mod file_roles;
pub mod policies;
pub mod preferences;
pub mod security_guidance;
pub mod suggestions;
pub mod suppression;
