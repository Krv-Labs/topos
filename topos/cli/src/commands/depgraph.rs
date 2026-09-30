//! `topos depgraph` — GitNexus dependency-graph generation for COMPOSABLE.
//!
//! Thin CLI wrapper over the same `generate_depgraph` / `depgraph_status`
//! paths the MCP `topos_generate_depgraph` tool uses. Restores the
//! `topos depgraph generate` entry point referenced by README, the VS Code
//! extension, and agent docs (issue #206).

mod generate;
mod store;

use clap::{Args, Subcommand};

use generate::{run_generate, GenerateArgs};

pub(crate) use generate::{gitnexus_available, prepare_pr_stores};
#[cfg(test)]
pub(crate) use store::write_commits;
pub(crate) use store::{build_estimate_ms, last_build_ms, pr_store_state, PrStores, StoreState};

#[derive(Args)]
pub struct DepgraphArgs {
    #[command(subcommand)]
    pub action: DepgraphAction,
}

#[derive(Subcommand)]
pub enum DepgraphAction {
    /// Build or refresh `.gitnexus/` via `gitnexus analyze --skip-agents-md`.
    Generate(GenerateArgs),
    /// Build the base and head graphs for one pull request, in parallel.
    /// Does not print the review.
    #[command(name = "generate-pr")]
    GeneratePr(generate::GeneratePrArgs),
}

pub fn run(args: DepgraphArgs) -> Result<(), String> {
    match args.action {
        DepgraphAction::Generate(args) => run_generate(args),
        DepgraphAction::GeneratePr(args) => generate::run_generate_pr(args),
    }
}

pub(super) fn print_json(value: &serde_json::Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(())
}
