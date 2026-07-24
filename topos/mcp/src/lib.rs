//! Topos MCP server library.
//!
//! Rust rewrite of the former `topos/mcp` Python package (PR #159): all
//! computation lives in `topos-core`; this crate is the MCP wire layer —
//! schemas, orchestration, formatting, and the stdio server.

pub mod diagnostics;
pub mod docs;
pub mod evaluation;
pub mod formatting;
pub mod metric_locations;
pub mod refactor_hotspots;
pub mod refactor_targets;
pub mod schemas;
pub mod security;
pub mod security_findings;
pub mod server;
pub mod sighthound;
pub mod snapshots;
pub mod tools;

#[cfg(test)]
mod context_budget;
