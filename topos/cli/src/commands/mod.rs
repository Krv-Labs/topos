//! One module per `topos` subcommand, plus a shared [`lang`] helper.

mod classify;
pub mod compare;
mod composable;
pub mod coverage;
pub mod depgraph;
pub mod evaluate;
pub mod graphify;
pub mod inspect;
mod lang;
pub mod mcp;
mod render;
