//! `topos graphify generate` — (re)generate `graphify-out/graph.json`.
//!
//! Split out of `graphify.rs` alongside [`super::orphans`] -- these are
//! two independent subcommands bundled under one CLI action, and kept
//! together were pushing the parent file's cyclomatic total over the
//! SIMPLE gate.

use std::path::PathBuf;

use clap::Args;
use topos_engine::adapters::graphify::{ensure_graphify_graph, GRAPHIFY_GRAPH_FILE};

use super::print_json;

#[derive(Args)]
pub struct GenerateArgs {
    /// Directory to analyze (default: current directory).
    pub path: Option<PathBuf>,
    /// Regenerate even when a graph is already present.
    #[arg(long)]
    pub force: bool,
    /// Output the result as a single JSON object.
    #[arg(long)]
    pub json: bool,
}

pub fn run_generate(args: GenerateArgs) -> Result<(), String> {
    let target_dir = args.path.unwrap_or_else(|| PathBuf::from("."));
    if !target_dir.is_dir() {
        return Err(format!("Not a directory: {}", target_dir.display()));
    }

    let graph_file =
        topos_engine::adapters::graphify::graphify_out_dir(&target_dir).join(GRAPHIFY_GRAPH_FILE);
    if !args.force && graph_file.is_file() {
        let message = format!("Graphify graph already current: {}", graph_file.display());
        if args.json {
            print_json(&serde_json::json!({
                "ok": true,
                "generated": false,
                "graphify_out_dir": graph_file.parent().map(|p| p.display().to_string()),
                "message": message,
            }))?;
        } else {
            println!("{message}");
        }
        return Ok(());
    }

    let result = ensure_graphify_graph(&target_dir, args.json, None);
    if args.json {
        print_json(&serde_json::json!({
            "ok": result.ok,
            "returncode": result.returncode,
            "generated": result.ok,
            "graphify_out_dir": result.graphify_out_dir.map(|p| p.display().to_string()),
            "message": result.message,
        }))?;
    }

    if result.ok {
        Ok(())
    } else {
        Err(result.message)
    }
}
