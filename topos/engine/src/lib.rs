//! # topos-engine
//!
//! Pure-Rust engine for Topos: category-theoretic evaluation of program
//! structure, driven by tree-sitter ASTs. Topos models a codebase as a
//! topos `E = Set^(C × H^op)` — its objects are program ASTs/graphs, its
//! morphisms are structure-preserving transformations (refactors), and its
//! subobject classifier `Ω` is the free Heyting algebra on three quality
//! generators (`SIMPLE`, `COMPOSABLE`, `SECURE`; see [`core::omega`]).
//!
//! This crate carries no MCP or CLI concerns and no Python bindings — it is
//! the shared compute engine both consumers link against: [`crate`] is
//! consumed by `topos` (the CLI binary) and `topos-mcp` (the MCP
//! server, also distributed to PyPI as a thin `bin` wheel that ships the
//! compiled server with no Python runtime).
//!
//! - [`core`] — the categorical primitives: `Object`, `Morphism`,
//!   `Category`, `Omega`, and the characteristic morphism `χ_S : P → Ω`.
//! - [`graphs`] — the `Representation` protocol and every structural view
//!   of a program (AST, CFG, CPG, PDG, MDG, UAST, process graphs).
//! - [`functors`] — probes (metrics over one representation) and
//!   profunctors (comparisons across two), including the Forman-Ricci
//!   curvature and CFG cycle-basis engines used by `topos refactor`.
//! - [`evaluation`] — the `Φᵢ` policy translators that feed `χ_S`,
//!   preference-ordered relaxation walks, and suppression/suggestion logic.
//! - [`adapters`] — filesystem/subprocess edges of the model: source-file
//!   discovery and GitNexus dependency-graph generation.
//! - [`config`] — the `.topos.toml` allowlist ([`config::ToposConfig`]),
//!   consumed by [`evaluation::suppression`].

pub mod adapters;
pub mod config;
pub mod core;
pub mod evaluation;
pub mod functors;
pub mod graphs;
