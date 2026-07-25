//! `topos graphify` — Graphify knowledge-graph generation and orphan
//! detection (issue #150).
//!
//! First CLI entry point for the refactor-suite tool family: unlike
//! `evaluate`/`inspect`/`compare`/`coverage`, `cycles`/`dependencies`/
//! `process` have no CLI subcommand today, only an MCP tool
//! (`topos_refactor`). `graphify` gets one because generating the graph
//! means shelling out to an external tool a human would want to watch run
//! interactively. Calls straight into the same `topos-engine` functions the
//! MCP tools use (`topos/mcp/src/tools/{graphify,refactor}.rs`) — no
//! logic duplicated between CLI and MCP.

mod generate;
mod orphans;

use clap::{Args, Subcommand};

use generate::{run_generate, GenerateArgs};
use orphans::{run_orphans, OrphansArgs};

#[derive(Args)]
pub struct GraphifyArgs {
    #[command(subcommand)]
    pub action: GraphifyAction,
}

#[derive(Subcommand)]
pub enum GraphifyAction {
    /// (Re)generate graphify-out/graph.json for a directory — invokes the
    /// external `graphify` CLI as a subprocess.
    Generate(GenerateArgs),
    /// List orphan nodes / fragile (INFERRED|AMBIGUOUS) edges for a file.
    Orphans(OrphansArgs),
}

pub fn run(args: GraphifyArgs) -> Result<(), String> {
    match args.action {
        GraphifyAction::Generate(args) => run_generate(args),
        GraphifyAction::Orphans(args) => run_orphans(args),
    }
}

pub(super) fn print_json(value: &serde_json::Value) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|e| e.to_string())?
    );
    Ok(())
}
