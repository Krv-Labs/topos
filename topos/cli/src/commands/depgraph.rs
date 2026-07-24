//! `topos depgraph` — GitNexus dependency-graph generation for COMPOSABLE.
//!
//! Thin CLI wrapper over the same `generate_depgraph` / `depgraph_status`
//! paths the MCP `topos_generate_depgraph` tool uses. Restores the
//! `topos depgraph generate` entry point referenced by README, the VS Code
//! extension, and agent docs (issue #206).

mod generate;

use clap::{Args, Subcommand};

use generate::{run_generate, GenerateArgs};

#[derive(Args)]
pub struct DepgraphArgs {
    #[command(subcommand)]
    pub action: DepgraphAction,
}

#[derive(Subcommand)]
pub enum DepgraphAction {
    /// Build or refresh `.gitnexus/` via `gitnexus analyze --skip-agents-md`.
    Generate(GenerateArgs),
}

pub fn run(args: DepgraphArgs) -> Result<(), String> {
    match args.action {
        DepgraphAction::Generate(args) => run_generate(args),
    }
}

pub(super) fn print_json(value: &serde_json::Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(())
}
