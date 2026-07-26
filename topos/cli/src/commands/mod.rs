//! One module per `topos` subcommand, plus a shared [`lang`] helper.

mod classify;
pub mod compare;
mod composable;
pub mod config;
pub mod coverage;
pub mod depgraph;
pub mod evaluate;
mod evaluate_info;
mod evaluate_info_render;
pub mod graphify;
pub mod inspect;
mod inspect_render;
mod lang;
pub mod mcp;
mod render;
mod summary;
