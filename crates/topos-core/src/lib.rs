//! # topos-core
//!
//! Pure-Rust engine for Topos: category-theoretic evaluation of program
//! structure. Topos models a codebase as a topos `E = Set^(C × H^op)` —
//! its objects are program ASTs/graphs, its morphisms are
//! structure-preserving transformations (refactors), and its subobject
//! classifier `Ω` is the free Heyting algebra on three quality
//! generators (`SIMPLE`, `COMPOSABLE`, `SECURE`; see [`core::omega`]).
//!
//! This crate carries no `pyo3` dependency — see `topos-pyo3` for Python
//! bindings and `topos-cli` for the standalone `topos` binary.
//!
//! Port target for the v0.4.0 migration (milestone "Release v0.4.0").
//! Modules land here incrementally from `topos/core`, `topos/graphs`,
//! `topos/evaluation`, and `topos/functors` — see the tracked
//! `rust-migration` issues (#141-#149).
//!
//! - [`core`] — the categorical primitives: `Object`, `Morphism`,
//!   `Category`, and `Omega`. Landed in issue #141.
//! - [`graphs`] — the `Representation` protocol and (eventually) every
//!   structural view of a program. `base` landed in #141; concrete
//!   representations land in #142/#143.
//! - [`adapters`] — filesystem/subprocess edges of the model: source-file
//!   discovery and GitNexus dependency-graph generation. Landed in #146.

pub mod adapters;
pub mod core;
pub mod evaluation;
pub mod functors;
pub mod graphs;
