//! Evaluation: the characteristic morphism `χ_S : P → Ω` and the policy
//! translators `Φᵢ` that feed it.
//!
//! Ported so far: [`characteristic_morphism`], [`policies`],
//! [`file_roles`], and the [`preferences::Generator`] enum. Remaining
//! within issue #144: the full `preferences::UserPreferences`
//! relaxation walk, `security_guidance`, `suggestions`, `suppression`.

pub mod characteristic_morphism;
pub mod file_roles;
pub mod policies;
pub mod preferences;
