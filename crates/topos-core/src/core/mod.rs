//! The categorical core of the program topos.
//!
//! - [`omega`] is `Ω`, the subobject classifier / value Heyting algebra.
//! - [`object`] is `ProgramObject`, the AST lifted into the category.
//! - [`morphism`] is `ProgramMorphism`, source code viewed as an arrow
//!   `A → B` between objects.
//! - [`category`] is `ProgramCategory`, tying the above together with
//!   composition and (eventually) the characteristic morphism `χ_S`.
//! - [`characteristic_morphism`] is `χ_S : P → Ω`, the classifying arrow
//!   into `Ω` that the policy translators in [`crate::evaluation`] feed.

pub mod category;
pub mod characteristic_morphism;
pub mod morphism;
pub mod object;
pub mod omega;
