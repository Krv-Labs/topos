//! One module per `topos` subcommand, plus a shared [`lang`] helper.

mod classify;
pub mod compare;
mod composable;
pub mod config;
pub mod coverage;
pub mod depgraph;
pub mod evaluate;
mod gh;
pub mod inspect;
pub mod install;
mod interaction;
mod lang;
pub mod mcp;
mod menu;
pub mod pr_recap;
mod render;
