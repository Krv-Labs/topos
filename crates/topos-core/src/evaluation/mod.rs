//! Evaluation: the characteristic morphism `χ_S : P → Ω` and the policy
//! translators `Φᵢ` that feed it.
//!
//! - [`characteristic_morphism`] — `χ_S : P → Ω` itself.
//! - [`policies`] — the `Φᵢ` translators and their calibration/gates.
//! - [`file_roles`] — structural exemptions (entrypoint modules, etc.).
//! - [`preferences`] — [`preferences::UserPreferences`], the strict total
//!   order over the three generators and its `Ω` relaxation walk.
//! - [`security_guidance`] — remediation prose/operations for dangerous
//!   APIs and taint flows.
//! - [`suggestions`] — turns a [`characteristic_morphism::ClassificationResult`]
//!   into actionable refactor suggestions.
//! - [`suppression`] — the allowlist overlay that computes an *adjusted*
//!   SECURE verdict on top of the canonical one (anti-gaming design).
//!
//! Issue #144 (`topos/evaluation`) is now fully landed.

pub mod characteristic_morphism;
pub mod file_roles;
pub mod policies;
pub mod preferences;
pub mod security_guidance;
pub mod suggestions;
pub mod suppression;
