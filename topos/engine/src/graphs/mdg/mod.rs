//! The Module Dependency Graph — the inter-module (import/call/inherit)
//! view parsed from GitNexus output, feeding the COMPOSABLE generator.

pub mod file_graph;
mod json;
mod ladybug;
pub mod models;
pub mod object;
pub mod split;
