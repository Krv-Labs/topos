//! `topos depgraph status` — report graph state and the false edges the
//! loader dropped.

use std::path::PathBuf;

use clap::Args;
use console::Style;
use topos_mcp::evaluation::depgraph_status;
use topos_mcp::tools::depgraph::{dropped_edge_counts, dropped_edges_line};

use super::print_json;
use crate::commands::render::{guide, guide_line, paint, RenderOptions};

#[derive(Args)]
pub struct StatusArgs {
    /// Project directory (default: current directory).
    pub path: Option<PathBuf>,
    /// Output the result as a single JSON object, including every dropped edge.
    #[arg(long)]
    pub json: bool,
}

pub fn run_status(args: StatusArgs) -> Result<(), String> {
    let target_dir = match args.path {
        Some(path) => path,
        None => std::env::current_dir().map_err(|e| format!("current directory: {e}"))?,
    };
    if !target_dir.is_dir() {
        return Err(format!("Not a directory: {}", target_dir.display()));
    }
    let _cache_lease = topos_mcp::cache::lease_managed_path(&target_dir)
        .map_err(|error| format!("could not lease the managed PR cache: {error}"))?;

    let status = depgraph_status(None, &target_dir, &target_dir.to_string_lossy());
    let loaded = matches!(status.state, "present" | "stale");
    let counts = loaded.then(|| dropped_edge_counts(&status.dropped_edges));

    if args.json {
        return print_json(&serde_json::json!({
            "state": status.state,
            "gitnexus_dir": status.gitnexus_dir,
            "detail": status.detail,
            "dropped_edge_counts": counts,
            "dropped_edges": status.dropped_edges,
        }));
    }

    let options = RenderOptions::stdout();
    println!(
        "{}",
        paint(
            format!("◇  Dependency graph {}", status.state.replace('_', " ")),
            Style::new().bold(),
            options,
        )
    );
    if let Some(dir) = &status.gitnexus_dir {
        println!("{}", guide_line(dir, Style::new().dim(), options));
    }
    if let Some(detail) = &status.detail {
        println!("{}", guide_line(detail, Style::new().dim(), options));
    }
    if let Some(counts) = &counts {
        println!(
            "{}",
            guide_line(dropped_edges_line(counts), Style::new().dim(), options)
        );
    }
    println!("{}", guide('└', options));
    Ok(())
}
